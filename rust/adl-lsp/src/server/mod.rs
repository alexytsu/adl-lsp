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

use crate::node::NodeKind;
use crate::parser::definition::{Definition, DefinitionLocation, UnresolvedImport};
use crate::parser::hover::Hover as HoverTrait;
use crate::parser::tree::Tree;
use crate::parser::{AdlParser, ParsedTree};
use crate::server::config::ServerConfig;

pub mod config;
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
    config: ServerConfig,
    state: AdlLanguageServerState,
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
            state: AdlLanguageServerState::default(),
        }
    }

    fn ingest_document(
        client: &mut ClientSocket,
        documents: &mut RwLockWriteGuard<HashMap<Url, String>>,
        trees: &mut RwLockWriteGuard<HashMap<Url, ParsedTree>>,
        parser: &mut MutexGuard<AdlParser>,
        uri: &Url,
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
                uri: uri.clone(),
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
            if let Some((identifier, _node)) = tree.get_user_defined_text(&position, contents) {
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
                    let possible_import_paths = modules::resolve_import(
                        &self.config.package_roots,
                        &uri,
                        &unresolved_import,
                    );
                    let parsed_imports: Vec<Option<(Url, String)>> = possible_import_paths
                        .iter()
                        .map(|uri| {
                            let contents = std::fs::read_to_string(uri.path()).ok()?;
                            Some((uri.clone(), contents))
                        })
                        .collect();

                    for parsed_import in parsed_imports.into_iter() {
                        if parsed_import.is_none() {
                            continue;
                        }
                        let (resolved_uri, contents) = parsed_import.unwrap();
                        debug!("Unresolved import: {unresolved_import:?}");
                        debug!("Resolved import: {resolved_uri:?}");

                        // Ingest the document if not already ingested
                        Self::ingest_document(
                            &mut self.client,
                            documents,
                            trees,
                            parser,
                            &resolved_uri,
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

        let tree = trees.get(&uri);
        if tree.is_none() {
            return Ok(None);
        }
        let tree = tree.unwrap();

        if let Some((identifier, node)) = tree.get_user_defined_text(
            &position,
            documents.get(&uri).unwrap_or(&"".into()).as_bytes(),
        ) {
            // debug!(
            //     "found identifier: {} of node_type {:?}",
            //     identifier,
            //     node.kind()
            // );

            // TODO: factor this logic into the ParsedTree impl
            let parent = node.parent();
            if parent.is_none() {
                return Ok(None);
            }
            let parent = parent.unwrap();
            // debug!("parent: {:?}", parent.kind());

            // could check if grandparent is struct_definition or union_definition etc.
            if !NodeKind::is_scoped_name(&parent) {
                return Ok(None);
            }

            // Find all definition locations for this identifier in the current file
            let definition_locations = tree.definition(
                identifier,
                documents.get(&uri).unwrap_or(&"".into()).as_bytes(),
            );

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

            if definition_locations.is_empty() {
                return Ok(None);
            }

            // Split into resolved and unresolved imports
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

            // Resolve the unresolved imports
            for unresolved_import in unresolved_imports {
                let possible_import_paths =
                    modules::resolve_import(&self.config.package_roots, &uri, &unresolved_import);
                let parsed_imports: Vec<Option<(Url, String)>> = possible_import_paths
                    .iter()
                    .map(|uri| {
                        let contents = std::fs::read_to_string(uri.path()).ok()?;
                        Some((uri.clone(), contents))
                    })
                    .collect();

                for parsed_import in parsed_imports.into_iter() {
                    if parsed_import.is_none() {
                        continue;
                    }
                    let (resolved_uri, contents) = parsed_import.unwrap();
                    debug!("Unresolved import: {unresolved_import:?}");
                    debug!("Resolved import: {resolved_uri:?}");
                    // TODO: check cache before re-ingesting
                    Self::ingest_document(
                        &mut self.client,
                        documents,
                        trees,
                        parser,
                        &resolved_uri,
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
                                // no nested imports? perhaps this can technically be supported by a recursive lookup
                                error!("import not resolved after one level of resolution");
                            }
                        }
                    }
                }
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
        Self::ingest_document(
            &mut self.client,
            &mut self.state.documents.write().expect("poisoned"),
            &mut self.state.trees.write().expect("poisoned"),
            &mut self.state.parser.lock().expect("poisoned"),
            &uri,
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
            &uri,
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
