use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_lsp::{ClientSocket, LanguageClient};
use lsp_types::{PublishDiagnosticsParams, Url};
use tracing::debug;

use crate::parser::{AdlParser, ParsedTree};
use crate::server::imports::{ImportManager, ImportsCache};

/// ADL Language Server state that manages documents and their parsed trees.
/// Provides atomic operations to ensure document content and tree are updated together.
#[derive(Default, Clone)]
pub struct AdlLanguageServerState {
    documents: Arc<RwLock<HashMap<Url, String>>>,
    trees: Arc<RwLock<HashMap<Url, ParsedTree>>>,
    import_manager: ImportsCache,
}

impl AdlLanguageServerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the target URI for an identifier from the imports table
    pub fn get_import_target(&self, identifier: &str) -> Option<Url> {
        self.import_manager.cache().lookup_identifier(identifier)
    }

    /// Atomically ingest a document, updating both content and parsed tree together.
    /// This ensures consistency between the document content and its AST representation.
    pub fn ingest_document(
        &self,
        client: &mut ClientSocket,
        parser: &mut AdlParser,
        package_roots: &[std::path::PathBuf],
        uri: &Url,
        contents: String,
    ) {
        debug!("Ingesting document: {uri:?}");

        // Parse the document first
        let parsed_tree = parser.parse(uri.clone(), contents.clone());

        // Acquire write locks for atomic update
        let mut documents = self.documents.write().expect("poisoned");
        let mut trees = self.trees.write().expect("poisoned");

        // Store document contents
        documents.insert(uri.clone(), contents.clone());

        // Store parsed tree and publish diagnostics
        if let Some(tree) = parsed_tree {
            let mut diagnostics = vec![];
            diagnostics.extend(tree.collect_parse_diagnostics());
            trees.insert(uri.clone(), tree.clone());

            // Resolve all imports into the imports table
            // This closure can parse documents that aren't already ingested
            let mut get_or_parse_document_tree = |target_uri: &Url| -> Option<ParsedTree> {
                // First try to get from already parsed trees
                if let Some(existing_tree) = trees.get(target_uri) {
                    return Some(existing_tree.clone());
                }

                // If not found, try to parse the file
                if let Ok(target_content) = std::fs::read_to_string(target_uri.path()) {
                    debug!(
                        "Parsing target document for import resolution: {}",
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
                package_roots,
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
}
