use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_lsp::router::Router;
use async_lsp::{ClientSocket, Error, ErrorCode, ResponseError};
use lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportResult, DocumentSymbolParams, DocumentSymbolResponse,
    FileOperationFilter, FileOperationPattern, FileOperationPatternKind,
    FileOperationRegistrationOptions, FullDocumentDiagnosticReport, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, Location, OneOf, ReferenceParams,
    RelatedFullDocumentDiagnosticReport, SaveOptions, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Url,
    WorkDoneProgressOptions, WorkspaceFileOperationsServerCapabilities,
    WorkspaceServerCapabilities,
};
use lsp_types::{notification, request};
use tracing::{debug, error, info, trace, warn};

use crate::node::NodeKind;
use crate::parser::definition::{Definition, DefinitionLocation};
use crate::parser::hover::Hover as HoverTrait;
use crate::parser::references::References;
use crate::parser::symbols::DocumentSymbols;
use crate::parser::{AdlParser, ParsedTree};
use crate::server::config::ServerConfig;
use crate::server::imports::Fqn;
use crate::server::modules::AdlPackageDefinition;
use crate::server::state::AdlLanguageServerState;

pub mod config;
mod imports;
mod modules;
mod state;

pub struct TickEvent;

#[derive(Clone)]
pub struct Server {
    client: ClientSocket,
    counter: i32,
    config: ServerConfig,
    state: AdlLanguageServerState,
    parser: Arc<Mutex<AdlParser>>,
}

const ADL_EXTENSIONS: [&str; 6] = ["adl", "java", "rs", "ts", "hs", "cpp"];

impl From<Server> for Router<Server> {
    fn from(server: Server) -> Self {
        let mut router = Router::new(server);

        router
            .request::<request::Initialize, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_initialize(params).await }
            })
            .request::<request::Shutdown, _>(|st, _| {
                let st = st.clone();
                async move { st.handle_shutdown().await }
            })
            .request::<request::HoverRequest, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_hover_request(params).await }
            })
            .request::<request::GotoDefinition, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_goto_definition(params) }
            })
            .request::<request::References, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_find_references(params) }
            })
            .request::<request::DocumentDiagnosticRequest, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_document_diagnostic_request(params) }
            })
            .request::<request::DocumentSymbolRequest, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_document_symbol_request(params) }
            })
            .notification::<notification::DidOpenTextDocument>(|st, params| {
                trace!("did open text document: {:?}", params);
                st.handle_did_open_text_document(params)
            })
            .notification::<notification::DidCloseTextDocument>(|st, params| {
                trace!("did close text document: {:?}", params);
                st.handle_did_close_text_document(params)
            })
            .notification::<notification::DidChangeTextDocument>(|st, params| {
                trace!("did change text document: {:?}", params);
                st.handle_did_change_text_document(params)
            })
            .notification::<notification::DidSaveTextDocument>(|st, params| {
                trace!("did save text document: {:?}", params);
                st.handle_did_save_text_document(params)
            })
            .notification::<notification::Exit>(|st, _| st.handle_exit())
            .notification::<notification::Initialized>(|_, _| ControlFlow::Continue(()))
            .notification::<notification::DidChangeConfiguration>(|st, params| {
                trace!("did change configuration: {:?}", params);
                st.handle_did_change_configuration(params)
            })
            .event::<TickEvent>(|st, _| st.handle_tick_event());

        router
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
            &self.config.search_dirs,
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
                // TODO: don't read from disk if it's already open (owned by client and should already be parsed)
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

    // TODO(alex): have this accept a AsRef<Path> instead of a &Path?
    fn package_root_from_path(path: &Path) -> Option<PathBuf> {
        if !path.exists() {
            None
        } else if path.is_dir() && path.join("adl-package.json").exists() {
            Some(path.to_path_buf())
        } else {
            path.parent().and_then(Self::package_root_from_path)
        }
    }

    /// Discover all .adl files in the search dirs and any dependencies specified in adl-package.json
    fn discover_adl_files(&self) -> Vec<PathBuf> {
        let mut adl_files = Vec::new();

        // Find the package roots from any search dirs
        let mut package_roots = HashSet::<PathBuf>::new();
        self.config.search_dirs.iter().for_each(|dir| {
            if let Some(root) = Self::package_root_from_path(dir.as_path()) {
                // resolve dependencies
                let package_definition_file = root.join("adl-package.json");
                if package_definition_file.exists() {
                    let package_json = std::fs::read_to_string(&package_definition_file).unwrap();
                    let package_definition: AdlPackageDefinition = serde_json::from_str(&package_json).unwrap();
                    info!("found package definition at {}: {:?}", &package_definition_file.display(), package_definition);
                    let depdency_roots = package_definition.dependencies.iter().map(|dep| {
                        // TODO(alex): better detection of relative v.s. absolute paths
                        let dep_root = root.join(&dep.localdir);
                        if dep_root.exists() {
                            dep_root
                        } else {
                            // treat as absolute path?
                            root.join(&dep.localdir)
                        }
                    });
                    // TODO(alex): this part needs to recursively discover dependencies in new roots
                    package_roots.extend(depdency_roots);
                }

                package_roots.insert(root);
            } else {
                // If no package root is found, add the explicit search dir as a package root
                package_roots.insert(dir.to_path_buf());
            }
        });

        for package_root in &package_roots {
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
                    // TODO: look for .gitignore files and also ignore them
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
                } else if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.to_lowercase().contains("adl"))
                {
                    debug!("found ADL file: {}", path.display());
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

    /// Handle the `initialize` notification and respond with the server's capabilities.
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

        for suffix in ADL_EXTENSIONS {
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
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        // Get notified on open and close of files (client has taken ownership)
                        open_close: Some(true),
                        // Request full changes on save
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        will_save: None,
                        will_save_wait_until: None, // NOTE: could be used to run autoformatting here, returning a list of edits
                        change: None, // Not responding per change until a more permissive grammar is integrated
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        inter_file_dependencies: true,
                        identifier: Some(String::from("adl-lsp")),
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                        workspace_diagnostics: false, // TODO: check that modules are defined in the correctly structured files
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

    fn resolve_import_from_table<F, T>(
        &mut self,
        import: &Fqn,
        mut process_import: F,
    ) -> Result<T, ResponseError>
    where
        F: FnMut(&ParsedTree, &str) -> Result<T, ResponseError>,
    {
        // get the target URI for this identifier from the global imports table
        if let Some(target_uri) = self.state.get_import_target(import) {
            debug!("resolved import {:?} to {}", import, target_uri.path());

            // Get the target document tree (parse if not already loaded)
            if let Some(target_tree) = self.get_or_parse_document(&target_uri) {
                if let Some(target_content) = self.state.get_document_content(&target_uri) {
                    debug!("processing import {:?} in target document", import);
                    return process_import(&target_tree, &target_content);
                } else {
                    error!("could not get content for target document: {}", target_uri);
                }
            } else {
                error!("could not parse target document: {}", target_uri);
            }
        } else {
            error!("no import found in table for identifier: {:?}", import);
        }

        Err(ResponseError::new(
            ErrorCode::INTERNAL_ERROR,
            "no import found in table",
        ))
    }

    /// Get a document tree, parsing it if not already loaded
    fn get_or_parse_document(&mut self, uri: &Url) -> Option<ParsedTree> {
        // First try to get from already parsed trees
        if let Some(existing_tree) = self.state.get_document_tree(uri) {
            return Some(existing_tree);
        }

        // If not found, try to parse the file
        if let Ok(content) = std::fs::read_to_string(uri.path()) {
            // TODO: don't read from disk if it's already open (owned by client and should already be parsed)
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
        // TODO: don't read from disk if it's already open (owned by client and should already be parsed)
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

        let contents = contents.as_bytes();
        let Some((identifier, _node)) = tree.get_identifier_at(&position, contents) else {
            return Ok(None);
        };
        let definition_location = tree.definition(identifier, contents);

        let mut hover_items = tree.hover(identifier, contents);
        match definition_location {
            Some(DefinitionLocation::Resolved(_location)) => {
                debug!("found local definition for {}", identifier);
            }
            Some(DefinitionLocation::Import(unresolved_import)) => {
                debug!(
                    "declaration for {} was imported from {}",
                    identifier,
                    unresolved_import.target_module_path.join(".")
                );
                let imported_hover_items = self.resolve_import_from_table(
                    &Fqn::from_module_name_and_type_name(
                        &unresolved_import.target_module_path.join("."),
                        identifier,
                    ),
                    |tree, contents| Ok(tree.hover(identifier, contents.as_bytes())),
                )?;
                hover_items.extend(imported_hover_items);
            }
            None => {
                error!("no definition found for {}", identifier);
            }
        };

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
            Some(DefinitionLocation::Import(unresolved_import)) => {
                debug!(
                    "declaration for {} was imported from {}",
                    identifier,
                    unresolved_import.target_module_path.join(".")
                );
                let resolved_import = self.resolve_import_from_table(
                    &Fqn::from_module_name_and_type_name(
                        &unresolved_import.target_module_path.join("."),
                        identifier,
                    ),
                    |tree, contents| {
                        let definition_location = tree.definition(identifier, contents.as_bytes());
                        match definition_location {
                            Some(DefinitionLocation::Resolved(location)) => Ok(Some(location)),
                            Some(DefinitionLocation::Import(_)) | None => {
                                error!("import not resolved after lookup in import table");
                                Err(ResponseError::new(
                                    ErrorCode::INTERNAL_ERROR,
                                    "import not resolved after lookup in import table",
                                ))
                            }
                        }
                    },
                )?;

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

        let contents = contents.as_bytes();
        let Some((identifier, node)) = tree.get_identifier_at(&position, contents) else {
            return Ok(None);
        };

        if !NodeKind::can_be_referenced(&node) {
            return Ok(None);
        }

        debug!("finding references for identifier: {}", identifier);

        let mut all_references = Vec::new();

        // First, find references in the current file
        let local_references = tree.find_references(identifier, contents);
        all_references.extend(local_references);
        let module_name = tree.get_module_name(contents).ok_or_else(|| {
            error!("could not get module name for document: {}", uri);
            ResponseError::new(ErrorCode::INTERNAL_ERROR, "could not get module name")
        })?;

        // Then, find all files that import this identifier
        let importing_files =
            self.state
                .get_files_importing_type(&Fqn::from_module_name_and_type_name(
                    module_name,
                    identifier,
                ));
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
            let definition_location = tree.definition(identifier, contents);
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
        let Some((tree, content)) = self.get_or_parse_document_with_content(&uri) else {
            return Err(ResponseError::new(
                ErrorCode::INVALID_REQUEST,
                "document not found",
            ));
        };

        let diagnostics = tree.collect_diagnostics(&content);

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
        error!("unexpected textDocument/didChange event");

        let uri = params.text_document.uri;
        let contents = params.content_changes.first();

        trace!("textDocument/didChange event for {}", uri.path());

        if let Some(change) = contents {
            let contents = change.text.clone();
            trace!("textDocument/didChange content is {:?}", change);
            self.ingest_document(&uri, contents);
        }

        ControlFlow::Continue(())
    }

    pub fn handle_did_save_text_document(
        &mut self,
        params: DidSaveTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        let uri = params.text_document.uri;
        if let Some(contents) = params.text {
            self.ingest_document(&uri, contents);
        } else {
            // NOTE: if there's no contents then this is unexpected but we could read the contents from the uri directly
            warn!("text file saved with no contents");
        }
        ControlFlow::Continue(())
    }

    pub fn handle_did_close_text_document(
        &mut self,
        _params: DidCloseTextDocumentParams,
    ) -> ControlFlow<Result<(), Error>> {
        ControlFlow::Continue(())
    }

    pub fn handle_did_change_configuration(
        &mut self,
        params: DidChangeConfigurationParams,
    ) -> ControlFlow<Result<(), Error>> {
        let search_dirs: Result<Vec<PathBuf>, ResponseError> = params
            .settings
            .get("searchDirs")
            .ok_or_else(|| ResponseError::new(ErrorCode::INTERNAL_ERROR, "searchDirs not found"))
            .map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| PathBuf::from(v.as_str().unwrap()))
                    .collect()
            });
        if let Ok(search_dirs) = search_dirs {
            self.config.search_dirs = search_dirs;
            self.state.clear_cache();
            self.initialize_workspace();
        }
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
