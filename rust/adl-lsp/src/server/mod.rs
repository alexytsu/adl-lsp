use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use async_lsp::router::Router;
use async_lsp::{ClientSocket, Error, ResponseError};
use lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReportPartialResult,
    DocumentDiagnosticReportResult, FileOperationFilter, FileOperationPattern,
    FileOperationPatternKind, FileOperationRegistrationOptions, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, Location, OneOf, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url, WorkDoneProgressOptions,
    WorkspaceFileOperationsServerCapabilities, WorkspaceServerCapabilities,
};
use tracing::{debug, error};

use crate::node::NodeKind;
use crate::parser::definition::{Definition, DefinitionLocation, UnresolvedImport};
use crate::parser::hover::Hover as HoverTrait;
use crate::parser::tree::Tree;
use crate::parser::{AdlParser, ParsedTree};
use crate::server::config::ServerConfig;
use crate::server::state::AdlLanguageServerState;

pub mod config;
mod imports;
mod modules;
mod state;

#[derive(Clone)]
pub struct Server {
    client: ClientSocket,
    counter: i32,
    config: ServerConfig,
    state: AdlLanguageServerState,
    parser: Arc<Mutex<AdlParser>>,
}

impl From<Server> for Router<Server> {
    fn from(server: Server) -> Self {
        Router::new(server)
    }
}

impl Server {
    pub fn new(client: &ClientSocket, config: ServerConfig) -> Self {
        Self {
            counter: 0,
            client: client.clone(),
            config,
            state: AdlLanguageServerState::new(),
            parser: Arc::new(Mutex::new(AdlParser::new())),
        }
    }

    fn ingest_document(&mut self, uri: &Url, contents: String) {
        let mut parser = self.parser.lock().expect("poisoned");
        self.state
            .ingest_document(&mut self.client, &mut parser, &self.config.package_roots, uri, contents);
    }

    pub async fn handle_shutdown(&self) -> Result<(), ResponseError> {
        // TODO: cleanup? after shutdown, should respond with InvalidRequest to all other requests
        debug!("Shutting down server");
        Ok(())
    }

    pub fn handle_exit(&self) -> ControlFlow<Result<(), Error>> {
        debug!("Exiting server");
        std::process::exit(0);
    }

    pub async fn handle_initialize(
        &self,
        _params: InitializeParams,
    ) -> Result<InitializeResult, ResponseError> {
        let file_operation_filers = vec![FileOperationFilter {
            scheme: Some(String::from("file")),
            pattern: FileOperationPattern {
                glob: String::from("**/*.{adl}"),
                matches: Some(FileOperationPatternKind::File),
                ..Default::default()
            },
        }];

        let file_registration_option = FileOperationRegistrationOptions {
            filters: file_operation_filers,
        };

        Ok(InitializeResult {
            // TODO: fine-tune these capabilities
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        will_save: None,
                        will_save_wait_until: None,
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        inter_file_dependencies: false,
                        workspace_diagnostics: false, // TODO: do file-structure diagnostics
                        identifier: None,
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: Some(false),
                        },
                    },
                )),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: None,
                    file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                        did_create: Some(file_registration_option.clone()),
                        will_create: Some(file_registration_option.clone()),
                        did_rename: Some(file_registration_option.clone()),
                        will_rename: Some(file_registration_option.clone()),
                        did_delete: Some(file_registration_option.clone()),
                        will_delete: Some(file_registration_option.clone()),
                    }),
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some({
                ServerInfo {
                    name: String::from("ADL Language Server"),
                    version: Some(String::from(env!("CARGO_PKG_VERSION"))),
                }
            }),
        })
    }


    fn resolve_imports_from_table<F, T>(
        &mut self,
        identifier: &str,
        mut process_import: F,
    ) -> Vec<T>
    where
        F: FnMut(&ParsedTree, &str, &str) -> Vec<T>,
    {
        let mut results = Vec::new();
        
        // Get the target URI for this identifier from the global imports table
        if let Some(target_uri) = self.state.get_import_target(identifier) {
            debug!("Resolving import from table: {} -> {}", identifier, target_uri);
            
            // Get the target document tree (parse if not already loaded)
            if let Some(target_tree) = self.get_or_parse_document(&target_uri) {
                if let Some(target_content) = self.state.get_document_content(&target_uri) {
                    debug!("Processing import for identifier '{}' in target document", identifier);
                    let imported_results = process_import(&target_tree, identifier, &target_content);
                    debug!("Import processing returned {} results", imported_results.len());
                    results.extend(imported_results);
                } else {
                    debug!("Could not get content for target document: {}", target_uri);
                }
            } else {
                debug!("Could not parse target document: {}", target_uri);
            }
        } else {
            debug!("No import found in table for identifier: {}", identifier);
        }
        
        results
    }

    /// Get a document tree, parsing it if not already loaded
    fn get_or_parse_document(&mut self, uri: &Url) -> Option<ParsedTree> {
        // First try to get from already parsed trees
        if let Some(existing_tree) = self.state.get_document_tree(uri) {
            return Some(existing_tree);
        }
        
        // If not found, try to parse the file
        if let Ok(content) = std::fs::read_to_string(uri.path()) {
            debug!("Parsing target document for LSP operation: {}", uri);
            self.ingest_document(uri, content);
            return self.state.get_document_tree(uri);
        }
        
        None
    }

    pub async fn handle_hover_request(
        &mut self,
        params: HoverParams,
    ) -> Result<Option<Hover>, ResponseError> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(tree) = self.state.get_document_tree(&uri) {
            if let Some(contents) = self.state.get_document_content(&uri) {
                let contents_bytes = contents.as_bytes();
                if let Some((identifier, _node)) =
                    tree.get_user_defined_text(&position, contents_bytes)
                {
                    let mut hover_items = tree.hover(identifier, contents_bytes);

                    debug!("Hovering over: {}", identifier);
                    debug!("Found hover items: {:?}", hover_items);

                    // Only check imports if no local definition was found
                    if hover_items.is_empty() {
                        let imported_hover_items = self.resolve_imports_from_table(
                            identifier,
                            |tree, identifier, contents| tree.hover(identifier, contents.as_bytes()),
                        );
                        hover_items.extend(imported_hover_items);
                    }

                    return Ok(Some(Hover {
                        contents: HoverContents::Array(hover_items),
                        range: None,
                    }));
                }
            }
        }

        Ok(None)
    }

    pub fn handle_goto_definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>, ResponseError> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let tree = self.state.get_document_tree(&uri);
        if tree.is_none() {
            return Ok(None);
        }
        let tree = tree.unwrap();

        let contents = self.state.get_document_content(&uri).unwrap_or_default();
        let contents_bytes = contents.as_bytes();

        if let Some((identifier, node)) = tree.get_user_defined_text(&position, contents_bytes) {
            let parent = node.parent();
            if parent.is_none() {
                return Ok(None);
            }
            let parent = parent.unwrap();

            if !NodeKind::is_scoped_name(&parent) {
                return Ok(None);
            }

            // Find all definition locations for this identifier in the current file
            let definition_locations = tree.definition(identifier, contents_bytes);

            debug!(
                "found definition locations: {:?}",
                &definition_locations
                    .iter()
                    .map(|d| match d {
                        DefinitionLocation::Resolved(location) => location.uri.to_string(),
                        DefinitionLocation::Import(unresolved_import) =>
                            format!("import from {:?}", unresolved_import.target_module_path),
                    })
                    .collect::<Vec<String>>()
            );

            // Split into resolved and unresolved imports
            let mut resolved_locations: Vec<Location> = vec![];
            let _unresolved_imports: Vec<UnresolvedImport> = definition_locations
                .into_iter()
                .filter_map(|loc| {
                    if let DefinitionLocation::Resolved(location) = loc {
                        resolved_locations.push(location);
                        None
                    } else if let DefinitionLocation::Import(imp) = loc {
                        Some(imp)
                    } else {
                        None
                    }
                })
                .collect();

            // Only check imports table if no local definition was found
            if resolved_locations.is_empty() {
                let imported_locations = self.resolve_imports_from_table(
                    identifier,
                    |tree, identifier, contents| {
                        tree.definition(identifier, contents.as_bytes())
                            .into_iter()
                            .filter_map(|loc| {
                                if let DefinitionLocation::Resolved(location) = loc {
                                    Some(location)
                                } else {
                                    error!("import not resolved after one level of resolution");
                                    None
                                }
                            })
                            .collect::<Vec<Location>>()
                    }
                );
                resolved_locations.extend(imported_locations);
            }

            if resolved_locations.is_empty() {
                return Ok(None);
            }

            Ok(Some(GotoDefinitionResponse::Array(resolved_locations)))
        } else {
            Ok(None)
        }
    }

    pub fn handle_document_diagnostic_request(
        &self,
        _params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult, ResponseError> {
        // TODO: implement this. currently we publish diagnostics on every change, so this is not needed
        Ok(DocumentDiagnosticReportResult::Partial(
            DocumentDiagnosticReportPartialResult {
                related_documents: None,
            },
        ))
    }
}

// Notifications
impl Server {
    pub fn handle_did_open_text_document(
        &mut self,
        params: DidOpenTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        let uri = params.text_document.uri;
        let contents = params.text_document.text;
        self.ingest_document(&uri, contents);
        ControlFlow::Continue(())
    }

    pub fn handle_did_change_text_document(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        let uri = params.text_document.uri;
        let contents = params.content_changes.first().unwrap().text.clone();
        self.ingest_document(&uri, contents);
        ControlFlow::Continue(())
    }
}

// Events
impl Server {
    pub fn handle_tick_event(&mut self) -> ControlFlow<Result<(), Error>> {
        self.counter += 1;
        ControlFlow::Continue(())
    }
}
