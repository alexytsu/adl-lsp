use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_lsp::{ClientSocket, LanguageClient};
use lsp_types::{PublishDiagnosticsParams, Url};
use tracing::debug;

use crate::parser::{AdlParser, ParsedTree};

/// ADL Language Server state that manages documents and their parsed trees.
/// Provides atomic operations to ensure document content and tree are updated together.
#[derive(Default, Clone)]
pub struct AdlLanguageServerState {
    documents: Arc<RwLock<HashMap<Url, String>>>,
    trees: Arc<RwLock<HashMap<Url, ParsedTree>>>,
}

impl AdlLanguageServerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Atomically ingest a document, updating both content and parsed tree together.
    /// This ensures consistency between the document content and its AST representation.
    pub fn ingest_document(
        &self,
        client: &mut ClientSocket,
        parser: &mut AdlParser,
        uri: &Url,
        contents: String,
    ) {
        debug!("Ingesting document: {uri:?}");

        // TODO: we need to expand all * imports so that GotoDefinition can find them

        // Parse the document first
        let parsed_tree = parser.parse(uri.clone(), contents.clone());

        // Acquire write locks for atomic update
        let mut documents = self.documents.write().expect("poisoned");
        let mut trees = self.trees.write().expect("poisoned");

        // Store document contents
        documents.insert(uri.clone(), contents);

        // Store parsed tree and publish diagnostics
        if let Some(tree) = parsed_tree {
            let mut diagnostics = vec![];
            diagnostics.extend(tree.collect_parse_diagnostics());
            trees.insert(uri.clone(), tree);
            
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
}