use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_lsp::router::Router;
use async_lsp::{ClientSocket, Error, ErrorCode, ResponseError};
use lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    DocumentSymbolParams, DocumentSymbolResponse, FileOperationFilter, FileOperationPattern,
    FileOperationPatternKind, FileOperationRegistrationOptions, FullDocumentDiagnosticReport,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, OneOf, ReferenceParams,
    RelatedFullDocumentDiagnosticReport, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url, WorkDoneProgressOptions,
    WorkspaceFileOperationsServerCapabilities, WorkspaceServerCapabilities,
};
use tracing::{debug, error};

use crate::node::NodeKind;
use crate::parser::definition::{Definition, DefinitionLocation};
use crate::parser::hover::Hover as HoverTrait;
use crate::parser::references::References;
use crate::parser::symbols::DocumentSymbols;
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
        self.state.ingest_document(
            &mut self.client,
            &mut parser,
            &self.config.package_roots,
            uri,
            contents,
        );
    }

    /// Initialize the server by discovering and processing all ADL files in package roots
    pub fn initialize_workspace(&mut self) {
        debug!("initializing workspace by discovering ADL files in package roots");

        let adl_files = self.discover_adl_files();
        debug!("found {} ADL files to preprocess", adl_files.len());

        for file_path in &adl_files {
            if let Ok(uri) = Url::from_file_path(file_path) {
                if let Ok(contents) = std::fs::read_to_string(file_path) {
                    debug!("preprocessing ADL file: {}", uri);
                    self.ingest_document(&uri, contents);
                } else {
                    error!("failed to read file: {}", file_path.display());
                }
            } else {
                error!("failed to convert path to URI: {}", file_path.display());
            }
        }

        debug!("workspace initialization complete");
        debug!("files processed: {}", &adl_files.len());
    }

    /// Discover all .adl files in the configured package roots
    fn discover_adl_files(&self) -> Vec<PathBuf> {
        let mut adl_files = Vec::new();

        for package_root in &self.config.package_roots {
            debug!(
                "discovering ADL files in package root: {}",
                package_root.display()
            );

            if package_root.exists() && package_root.is_dir() {
                Self::discover_adl_files_recursive(package_root, &mut adl_files);
            } else {
                debug!(
                    "package root does not exist or is not a directory: {}",
                    package_root.display()
                );
            }
        }

        debug!("total ADL files discovered: {}", adl_files.len());
        adl_files
    }

    /// Recursively discover .adl files in a directory
    fn discover_adl_files_recursive(dir: &PathBuf, adl_files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_dir() {
                    // Skip hidden directories and common build/output directories
                    if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                        if !dir_name.starts_with('.')
                            && dir_name != "target"
                            && dir_name != "node_modules"
                            && dir_name != "dist"
                            && dir_name != "build"
                        {
                            Self::discover_adl_files_recursive(&path, adl_files);
                        }
                    }
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("adl") {
                    debug!("Found ADL file: {}", path.display());
                    adl_files.push(path);
                }
            }
        }
    }

    pub async fn handle_shutdown(&self) -> Result<(), ResponseError> {
        // TODO: cleanup? after shutdown, should respond with InvalidRequest to all other requests
        debug!("shutting down server");
        Ok(())
    }

    pub fn handle_exit(&self) -> ControlFlow<Result<(), Error>> {
        debug!("exiting server");
        std::process::exit(0);
    }

    pub async fn handle_initialize(
        &mut self,
        _params: InitializeParams,
    ) -> Result<InitializeResult, ResponseError> {
        let mut file_operation_filers = vec![FileOperationFilter {
            scheme: Some(String::from("file")),
            pattern: FileOperationPattern {
                glob: String::from("**/*.{adl}"),
                matches: Some(FileOperationPatternKind::File),
                ..Default::default()
            },
        }];

        let suffixes = vec![
            String::from("java"),
            String::from("rs"),
            String::from("ts"),
            String::from("hs"),
            String::from("cpp"),
        ];

        for suffix in suffixes {
            file_operation_filers.push(FileOperationFilter {
                scheme: Some(String::from("file")),
                pattern: FileOperationPattern {
                    glob: format!("**/*.adl-{}", suffix),
                    matches: Some(FileOperationPatternKind::File),
                    ..Default::default()
                },
            })
        }

        let file_registration_option = FileOperationRegistrationOptions {
            filters: file_operation_filers,
        };

        let result = InitializeResult {
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
                references_provider: Some(OneOf::Left(true)),
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
                document_symbol_provider: Some(OneOf::Left(true)),
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
        };

        // Initialize workspace by discovering and processing all ADL files
        debug!("starting workspace initialization during LSP initialize");
        self.initialize_workspace();
        debug!("workspace initialization completed during LSP initialize");

        Ok(result)
    }

    fn resolve_import_from_table<F, T: Default>(
        &mut self,
        identifier: &str,
        mut process_import: F,
    ) -> T
    where
        F: FnMut(&ParsedTree, &str) -> T,
    {
        // get the target URI for this identifier from the global imports table
        if let Some(target_uri) = self.state.get_import_target(identifier) {
            debug!(
                "resolving import target location: {} -> {}",
                identifier,
                target_uri.path()
            );

            // Get the target document tree (parse if not already loaded)
            if let Some(target_tree) = self.get_or_parse_document(&target_uri) {
                if let Some(target_content) = self.state.get_document_content(&target_uri) {
                    debug!(
                        "processing import for identifier '{}' in target document",
                        identifier
                    );
                    return process_import(&target_tree, &target_content);
                } else {
                    error!("could not get content for target document: {}", target_uri);
                }
            } else {
                error!("could not parse target document: {}", target_uri);
            }
        } else {
            error!("no import found in table for identifier: {}", identifier);
        }

        T::default()
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

    /// Get document tree and content together, parsing if not already loaded
    fn get_or_parse_document_with_content(&mut self, uri: &Url) -> Option<(ParsedTree, String)> {
        // First try to get both atomically from already parsed documents
        if let Some(result) = self.state.get_document_tree_and_content(uri) {
            return Some(result);
        }

        // If not found, try to parse the file
        if let Ok(content) = std::fs::read_to_string(uri.path()) {
            debug!("Parsing target document for LSP operation: {}", uri);
            self.ingest_document(uri, content.clone());
            return self.state.get_document_tree_and_content(uri);
        }

        None
    }

    pub async fn handle_hover_request(
        &mut self,
        params: HoverParams,
    ) -> Result<Option<Hover>, ResponseError> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some((tree, contents)) = self.get_or_parse_document_with_content(&uri) else {
            return Ok(None);
        };

        let contents_bytes = contents.as_bytes();
        let Some((identifier, _node)) = tree.get_identifier_at(&position, contents_bytes) else {
            return Ok(None);
        };

        let mut hover_items = tree.hover(identifier, contents_bytes);

        // check imports if no local definition was found
        if hover_items.is_empty() {
            let imported_hover_items = self
                .resolve_import_from_table(identifier, |tree, contents| {
                    tree.hover(identifier, contents.as_bytes())
                });
            hover_items.extend(imported_hover_items);
        }

        Ok(Some(Hover {
            contents: HoverContents::Array(hover_items),
            range: None,
        }))
    }

    pub fn handle_goto_definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>, ResponseError> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some((tree, content)) = self.get_or_parse_document_with_content(&uri) else {
            return Ok(None);
        };

        let content = content.as_bytes();
        let Some((identifier, node)) = tree.get_identifier_at(&position, content) else {
            return Ok(None);
        };

        // identifiers appearing in scoped names reference a type_definition elsewhere
        if !NodeKind::has_scoped_name_parent(&node) {
            return Ok(None);
        }

        // search for a definition location in the current file
        let definition_location = tree.definition(identifier, content);

        match definition_location {
            Some(DefinitionLocation::Resolved(location)) => {
                debug!("found local definition for {}", identifier);
                Ok(Some(GotoDefinitionResponse::Scalar(location)))
            }
            Some(DefinitionLocation::Import(_unresolved_import)) => {
                // TODO: _unresolved import doesn't really need any information on it, since using the import table allows us to resolve via the identifier name
                debug!(
                    "declaration for {} was imported from {}",
                    identifier,
                    _unresolved_import.target_module_path.join(".")
                );
                let resolved_import =
                    self.resolve_import_from_table(identifier, |tree, contents| {
                        let definition_location = tree.definition(identifier, contents.as_bytes());
                        match definition_location {
                            Some(DefinitionLocation::Resolved(location)) => Some(location),
                            // if resolved import not found, use _unresolved_import to parse the target document
                            // however, this shouldn't happen, because we already built the import table when we parsed the source document
                            Some(DefinitionLocation::Import(_)) | None => {
                                error!("import not resolved after lookup in import table");
                                None
                            }
                        }
                    });

                Ok(resolved_import.map(GotoDefinitionResponse::Scalar))
            }
            None => Ok(None),
        }
    }

    pub fn handle_find_references(
        &mut self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>, ResponseError> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some((tree, contents)) = self.get_or_parse_document_with_content(&uri) else {
            return Ok(None);
        };

        let contents_bytes = contents.as_bytes();
        let Some((identifier, node)) = tree.get_identifier_at(&position, contents_bytes) else {
            return Ok(None);
        };

        if !NodeKind::can_be_referenced(&node) {
            return Ok(None);
        }

        debug!("finding references for identifier: {}", identifier);

        let mut all_references = Vec::new();

        // First, find references in the current file
        let local_references = tree.find_references(identifier, contents_bytes);
        all_references.extend(local_references);

        // Then, find all files that import this identifier
        let importing_files = self.state.get_files_importing_identifier(identifier);
        debug!(
            "found {} files importing identifier '{}'",
            importing_files.len(),
            identifier
        );

        // Parse each importing file and find references
        for importing_file_uri in importing_files {
            if importing_file_uri == uri {
                // Skip the current file, already processed above
                continue;
            }

            debug!(
                "checking for references in importing file: {}",
                importing_file_uri
            );

            // Get or parse the importing file
            if let Some(importing_tree) = self.get_or_parse_document(&importing_file_uri) {
                if let Some(importing_content) =
                    self.state.get_document_content(&importing_file_uri)
                {
                    let imported_references =
                        importing_tree.find_references(identifier, importing_content.as_bytes());
                    debug!(
                        "Found {} references in file {}",
                        imported_references.len(),
                        importing_file_uri
                    );
                    all_references.extend(imported_references);
                }
            }
        }

        // Include definition if requested
        if params.context.include_declaration {
            let definition_location = tree.definition(identifier, contents_bytes);
            if let Some(DefinitionLocation::Resolved(location)) = definition_location {
                all_references.push(location);
            }
        }

        debug!("Total references found: {}", all_references.len());

        if all_references.is_empty() {
            Ok(None)
        } else {
            Ok(Some(all_references))
        }
    }

    pub fn handle_document_diagnostic_request(
        &mut self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult, ResponseError> {
        let uri = params.text_document.uri;
        let Some(tree) = self.get_or_parse_document(&uri) else {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "document not found",
            ));
        };

        let diagnostics = tree.collect_diagnostics();

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    items: diagnostics,
                    result_id: None,
                },
            }),
        ))
    }

    pub fn handle_document_symbol_request(
        &mut self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>, ResponseError> {
        let uri = params.text_document.uri;

        // First try to get cached symbols
        if let Some(cached_symbols) = self.state.get_cached_document_symbols(&uri) {
            if cached_symbols.is_empty() {
                return Ok(None);
            } else {
                return Ok(Some(DocumentSymbolResponse::Nested(cached_symbols)));
            }
        }

        // If not cached, parse and compute symbols
        let Some((tree, content)) = self.get_or_parse_document_with_content(&uri) else {
            return Ok(None);
        };

        let symbols = tree.collect_document_symbols(content.as_bytes());

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(symbols)))
        }
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

    pub fn handle_did_save_text_document(
        &mut self,
        params: DidSaveTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        let uri = params.text_document.uri;
        if let Some(contents) = params.text {
            self.ingest_document(&uri, contents);
        }
        ControlFlow::Continue(())
    }

    pub fn handle_did_close_text_document(
        &mut self,
        _params: DidCloseTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
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
