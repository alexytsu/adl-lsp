use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use lsp_types::{Diagnostic, DocumentSymbol, Url};
use tracing::debug;

use crate::parser::symbols::DocumentSymbols;
use crate::parser::{AdlParser, ParsedTree};
use crate::server::imports::{Fqn, ImportManager, ImportsCache};
use crate::server::packages;

/// ADL Language Server state that manages documents and their parsed trees.
/// Provides atomic operations to ensure document content and tree are updated together.
#[derive(Default, Clone)]
pub struct AdlLanguageServerState {
    adl_file_to_package_root: Arc<RwLock<HashMap<Url, PathBuf>>>,
    package_root_to_adl_files: Arc<RwLock<HashMap<PathBuf, HashSet<Url>>>>,

    documents: Arc<RwLock<HashMap<Url, String>>>,
    trees: Arc<RwLock<HashMap<Url, ParsedTree>>>,

    symbols: Arc<RwLock<HashMap<Url, Vec<DocumentSymbol>>>>,
    import_manager: ImportsCache,
}

impl AdlLanguageServerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Atomically ingest a document, updating both content and parsed tree together.
    /// This ensures consistency between the document content and its AST representation.
    ///
    /// Returns the diagnostics collected for this document so the caller (the server layer,
    /// which owns the client handle) can publish them. Returns `None` if parsing failed.
    pub fn ingest_document(
        &self,
        parser: &mut AdlParser,
        uri: &Url,
        contents: String,
    ) -> Option<Vec<Diagnostic>> {
        debug!("ingesting document: {uri:?}");

        let parsed_tree = parser.parse(uri.clone(), &contents)?;

        debug!("collecting diagnostics on parse tree for {}", uri.path());
        let mut diagnostics = parsed_tree.collect_diagnostics(&contents);

        let symbols = parsed_tree.collect_document_symbols(contents.as_bytes());
        let mut symbols_cache = self.symbols.write().expect("poisoned");
        symbols_cache.insert(uri.clone(), symbols);

        let mut adl_file_to_package_root = self.adl_file_to_package_root.write().expect("poisoned");
        let mut package_root_to_adl_files =
            self.package_root_to_adl_files.write().expect("poisoned");

        let mut documents = self.documents.write().expect("poisoned");
        let mut trees = self.trees.write().expect("poisoned");

        // Prefer an `adl-package.json` marker; otherwise derive the root from the module name by
        // walking up the filesystem (module `a.b.c` in `<root>/a/b/c.adl` implies `<root>`).
        let package_root = packages::find_package_root_by_marker(uri.path()).or_else(|| {
            parsed_tree
                .find_module_name(contents.as_bytes())
                .and_then(|module_name| packages::package_root_from_module(uri.path(), module_name))
        });
        if let Some(package_root) = package_root {
            adl_file_to_package_root.insert(uri.clone(), package_root.clone());
            package_root_to_adl_files
                .entry(package_root)
                .or_default()
                .insert(uri.clone());
        }

        // pass closure allowing import_manager to recursively `resolve_and_register_imports`
        // alternative may be to use a queue here and have each call of resolve_and_register_imports
        // chain further files to parse
        let mut get_or_parse_document_tree = |target_uri: &Url| -> Option<ParsedTree> {
            if let Some(existing_tree) = trees.get(target_uri) {
                return Some(existing_tree.clone());
            }

            // If not found, try to parse the file
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

        self.import_manager.resolve_and_register_imports(
            &package_root_to_adl_files,
            uri,
            &parsed_tree,
            contents.as_bytes(),
            &mut get_or_parse_document_tree,
        );

        let invalid_import_diagnostics = self.import_manager.collect_invalid_import_diagnostics(
            &package_root_to_adl_files,
            uri,
            &parsed_tree,
            contents.as_bytes(),
            &mut get_or_parse_document_tree,
        );
        diagnostics.extend(invalid_import_diagnostics);

        // Store document contents
        documents.insert(uri.clone(), contents);
        trees.insert(uri.clone(), parsed_tree.clone());

        // The state layer only collects diagnostics; publishing is the server layer's
        // responsibility since it owns the client handle.
        Some(diagnostics)
    }

    pub fn clear_cache(&mut self) {
        self.adl_file_to_package_root
            .write()
            .expect("poisoned")
            .clear();
        self.package_root_to_adl_files
            .write()
            .expect("poisoned")
            .clear();
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
        self.import_manager.cache().lookup_files_that_import(fqn)
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

    /// Get access to the imports cache for completion and other operations
    pub fn get_imports_cache(&self) -> &ImportsCache {
        self.import_manager.cache()
    }
}
