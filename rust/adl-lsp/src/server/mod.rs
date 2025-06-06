use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard};

use async_lsp::router::Router;
use async_lsp::{ClientSocket, Error, LanguageClient, ResponseError};
use lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReportPartialResult,
    DocumentDiagnosticReportResult, FileOperationFilter, FileOperationPattern,
    FileOperationPatternKind, FileOperationRegistrationOptions, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, Location, OneOf, PublishDiagnosticsParams,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url, WorkDoneProgressOptions,
    WorkspaceFileOperationsServerCapabilities, WorkspaceServerCapabilities,
};
use tracing::{debug, error};

use crate::parser::definition::{Definition, DefinitionLocation, UnresolvedImport};
use crate::parser::hover::Hover as HoverTrait;
use crate::parser::tree::Tree;
use crate::parser::{AdlParser, ParsedTree};

mod modules;

#[derive(Default, Clone)]
struct AdlLanguageServerState {
    documents: std::sync::Arc<RwLock<HashMap<Url, String>>>,
    trees: Arc<RwLock<HashMap<Url, ParsedTree>>>,
    parser: Arc<Mutex<AdlParser>>,
    // TODO: store expanded imports for each Url
}

#[derive(Clone)]
pub struct Server {
    client: ClientSocket,
    counter: i32,
    state: AdlLanguageServerState,
}

impl From<Server> for Router<Server> {
    fn from(server: Server) -> Self {
        Router::new(server)
    }
}

impl Server {
    pub fn new(client: ClientSocket) -> Self {
        Self {
            client,
            counter: 0,
            state: AdlLanguageServerState::default(),
        }
    }

    fn ingest_document(
        client: &mut ClientSocket,
        documents: &mut RwLockWriteGuard<HashMap<Url, String>>,
        trees: &mut RwLockWriteGuard<HashMap<Url, ParsedTree>>,
        parser: &mut MutexGuard<AdlParser>,
        uri: Url,
        contents: String,
    ) {
        debug!("Ingesting document: {uri:?}");

        // TODO: we need to expand all * imports so that GotoDefinition can find them

        // Store document contents
        documents.insert(uri.clone(), contents.clone());

        // Parse and store AST
        let tree = parser.parse(uri.clone(), contents);

        if let Some(tree) = tree {
            let mut d = vec![];
            d.extend(tree.collect_parse_diagnostics());
            trees.insert(uri.clone(), tree);
            // TODO: handle error
            let _ = client.publish_diagnostics(PublishDiagnosticsParams {
                uri,
                diagnostics: d,
                version: None,
            });
        }
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
        params: InitializeParams,
    ) -> Result<InitializeResult, ResponseError> {
        debug!("Initialized with {params:#?}");

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
            server_info: None,
        })
    }

    pub async fn handle_hover_request(
        &mut self,
        params: HoverParams,
    ) -> Result<Option<Hover>, ResponseError> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let trees = &mut self.state.trees.write().expect("poisoned");
        let documents = &mut self.state.documents.write().expect("poisoned");
        let parser = &mut self.state.parser.lock().expect("poisoned");

        if let Some(tree) = trees.get(&uri) {
            let contents = documents.get(&uri).unwrap().as_bytes();
            if let Some(identifier) = tree.get_user_defined_text(&position, contents) {
                let mut hover_items = tree.hover(identifier, contents);

                debug!("Hovering over: {}", identifier);
                debug!("Found hover items: {:?}", hover_items);

                // Get definition locations to find imports
                let definition_locations = tree.definition(identifier, contents);
                let mut unresolved_imports: Vec<UnresolvedImport> = vec![];

                for definition_location in definition_locations {
                    if let DefinitionLocation::Import(unresolved_import) = definition_location {
                        unresolved_imports.push(unresolved_import);
                    }
                }

                // Handle imports
                for unresolved_import in unresolved_imports {
                    if let Some(resolved_uri) = modules::resolve_import(&uri, &unresolved_import) {
                        debug!("Unresolved import: {unresolved_import:?}");
                        debug!("Resolved import: {resolved_uri:?}");

                        // Read the contents of the resolved uri
                        let contents = std::fs::read_to_string(resolved_uri.path());
                        if let Err(e) = contents {
                            error!("Failed to read contents of {resolved_uri:?}: {e:?}");
                            continue;
                        }
                        let contents = contents.unwrap();

                        // Ingest the document if not already ingested
                        Self::ingest_document(
                            &mut self.client,
                            documents,
                            trees,
                            parser,
                            resolved_uri.clone(),
                            contents.clone(),
                        );

                        let imported_tree = trees.get(&resolved_uri).unwrap();
                        let imported_hover_items =
                            imported_tree.hover(&unresolved_import.identifier, contents.as_bytes());
                        hover_items.extend(imported_hover_items);
                    }
                }

                return Ok(Some(Hover {
                    contents: HoverContents::Array(hover_items),
                    range: None,
                }));
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

        let trees = &mut self.state.trees.write().expect("poisoned");
        let documents = &mut self.state.documents.write().expect("poisoned");
        let parser = &mut self.state.parser.lock().expect("poisoned");

        match trees.get(&uri) {
            Some(tree) => {
                // Get the identifier at the current position
                if let Some(identifier) =
                    tree.get_user_defined_text(&position, documents.get(&uri).unwrap().as_bytes())
                {
                    debug!("Found identifier: {}", identifier);

                    // Find all definition locations for this identifier
                    let definition_locations =
                        tree.definition(identifier, documents.get(&uri).unwrap().as_bytes());
                    debug!("Found definition locations: {:?}", definition_locations);

                    if !definition_locations.is_empty() {
                        let mut resolved_locations: Vec<Location> = vec![];
                        let mut unresolved_imports: Vec<UnresolvedImport> = vec![];

                        for definition_location in definition_locations {
                            match definition_location {
                                DefinitionLocation::Resolved(location) => {
                                    resolved_locations.push(location);
                                }
                                DefinitionLocation::Import(unresolved_import) => {
                                    unresolved_imports.push(unresolved_import);
                                }
                            }
                        }

                        for unresolved_import in unresolved_imports {
                            if let Some(resolved_uri) =
                                modules::resolve_import(&uri, &unresolved_import)
                            {
                                debug!("Unresolved import: {unresolved_import:?}");
                                debug!("Resolved import: {resolved_uri:?}");
                                // read the contents of the resolved uri manually
                                let contents = std::fs::read_to_string(resolved_uri.path());
                                if let Err(e) = contents {
                                    error!("Failed to read contents of {resolved_uri:?}: {e:?}");
                                    continue;
                                }
                                let contents = contents.unwrap();

                                // TODO: don't do this if we've already ingested the document
                                Self::ingest_document(
                                    &mut self.client,
                                    documents,
                                    trees,
                                    parser,
                                    resolved_uri.clone(),
                                    contents.clone(),
                                );

                                let imported_tree = trees.get(&resolved_uri).unwrap();

                                let definition_locations = imported_tree
                                    .definition(&unresolved_import.identifier, contents.as_bytes());
                                for definition_location in definition_locations {
                                    match definition_location {
                                        DefinitionLocation::Resolved(location) => {
                                            resolved_locations.push(location);
                                        }
                                        DefinitionLocation::Import(_) => {
                                            // no nested imports
                                        }
                                    }
                                }
                            }
                        }

                        Ok(Some(GotoDefinitionResponse::Array(resolved_locations)))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
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
        Self::ingest_document(
            &mut self.client,
            &mut self.state.documents.write().expect("poisoned"),
            &mut self.state.trees.write().expect("poisoned"),
            &mut self.state.parser.lock().expect("poisoned"),
            uri,
            contents,
        );
        ControlFlow::Continue(())
    }

    pub fn handle_did_change_text_document(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        let uri = params.text_document.uri;
        let contents = params.content_changes.first().unwrap().text.clone();
        Self::ingest_document(
            &mut self.client,
            &mut self.state.documents.write().expect("poisoned"),
            &mut self.state.trees.write().expect("poisoned"),
            &mut self.state.parser.lock().expect("poisoned"),
            uri,
            contents,
        );
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
