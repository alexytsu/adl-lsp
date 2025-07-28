use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use async_lsp::{ClientSocket, LanguageClient};
use lsp_types::{DocumentSymbol, PublishDiagnosticsParams, Url};
use tracing::debug;

use crate::parser::symbols::DocumentSymbols;
use crate::parser::{AdlParser, ParsedTree};
use crate::server::imports::{Fqn, ImportManager, ImportsCache};

/// ADL Language Server state that manages documents and their parsed trees.
/// Provides atomic operations to ensure document content and tree are updated together.
#[derive(Default, Clone)]
pub struct AdlLanguageServerState {
    documents: Arc<RwLock<HashMap<Url, String>>>,
    trees: Arc<RwLock<HashMap<Url, ParsedTree>>>,
    symbols: Arc<RwLock<HashMap<Url, Vec<DocumentSymbol>>>>,
    /// Map of directory URIs to the set of modules in that directory
    dir_modules: Arc<RwLock<HashMap<Url, HashSet<Url>>>>,
    /// Map of module URIs to the directory they belong to
    module_dirs: Arc<RwLock<HashMap<Url, Url>>>,
    import_manager: ImportsCache,
}

impl AdlLanguageServerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_cache(&mut self) {
        self.documents.write().expect("poisoned").clear();
        self.trees.write().expect("poisoned").clear();
        self.symbols.write().expect("poisoned").clear();
        self.import_manager.clear_cache();
    }

    /// Get the target URI for an identifier from the imports table
    pub fn get_import_target(&self, fqn: &Fqn) -> Option<Url> {
        self.import_manager.cache().lookup_fqn(fqn)
    }

    /// Get all files that import a specific type
    pub fn get_files_importing_type(&self, fqn: &Fqn) -> Vec<Url> {
        self.import_manager.cache().get_files_importing_type(fqn)
    }

    /// Atomically ingest a document, updating both content and parsed tree together.
    /// This ensures consistency between the document content and its AST representation.
    pub fn ingest_document(
        &self,
        client: &mut ClientSocket,
        parser: &mut AdlParser,
        search_dirs: &[std::path::PathBuf],
        uri: &Url,
        contents: String,
    ) {
        debug!("ingesting document: {uri:?}");

        // Parse the document first
        let parsed_tree = parser.parse(uri.clone(), contents.clone());

        // Acquire write locks for atomic update
        let mut documents = self.documents.write().expect("poisoned");
        let mut trees = self.trees.write().expect("poisoned");

        // Store document contents
        documents.insert(uri.clone(), contents.clone());

        if let Some(tree) = parsed_tree {
            // Collect diagnostics from the tree
            debug!("collecting diagnostics on parse tree for {}", uri.path());
            let diagnostics = tree.collect_diagnostics(&contents);
            trees.insert(uri.clone(), tree.clone());

            // Cache document symbols
            let symbols = tree.collect_document_symbols(contents.as_bytes());
            let mut symbols_cache = self.symbols.write().expect("poisoned");
            symbols_cache.insert(uri.clone(), symbols);

            let mut get_or_parse_document_tree = |target_uri: &Url| -> Option<ParsedTree> {
                // First try to get from already parsed trees
                if let Some(existing_tree) = trees.get(target_uri) {
                    return Some(existing_tree.clone());
                }

                // If not found, try to parse the file
                // TODO: don't read from disk if it's already open (owned by client and should already be parsed)
                if let Ok(target_content) = std::fs::read_to_string(target_uri.path()) {
                    debug!(
                        "parsing target document for import resolution: {}",
                        target_uri
                    );
                    if let Some(parsed_tree) =
                        parser.parse(target_uri.clone(), target_content.as_bytes())
                    {
                        // Store it for future use
                        trees.insert(target_uri.clone(), parsed_tree.clone());
                        documents.insert(target_uri.clone(), target_content);
                        return Some(parsed_tree);
                    }
                }
                None
            };

            self.import_manager.resolve_document_imports(
                search_dirs,
                uri,
                &tree,
                contents.as_bytes(),
                &mut get_or_parse_document_tree,
            );

            // TODO: handle error
            let _ = client.publish_diagnostics(PublishDiagnosticsParams {
                uri: uri.clone(),
                diagnostics,
                version: None,
            });
        }
    }

    /// Get the content of a document if it exists
    pub fn get_document_content(&self, uri: &Url) -> Option<String> {
        self.documents.read().expect("poisoned").get(uri).cloned()
    }

    /// Get a parsed tree for a document if it exists
    pub fn get_document_tree(&self, uri: &Url) -> Option<ParsedTree> {
        self.trees.read().expect("poisoned").get(uri).cloned()
    }

    /// Atomically get both document content and parsed tree
    pub fn get_document_tree_and_content(&self, uri: &Url) -> Option<(ParsedTree, String)> {
        let documents = self.documents.read().expect("poisoned");
        let trees = self.trees.read().expect("poisoned");

        match (trees.get(uri), documents.get(uri)) {
            (Some(tree), Some(content)) => Some((tree.clone(), content.clone())),
            _ => None,
        }
    }

    /// Get cached document symbols if available
    pub fn get_cached_document_symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        self.symbols.read().expect("poisoned").get(uri).cloned()
    }
}
