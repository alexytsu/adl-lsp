use std::path::PathBuf;

use crate::cli::{Cli, LspClient};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    _lsp_client: Option<LspClient>,
    /// Search dirs for adl packages specified by the user - does not include dependencies resolved from adl-package.json
    pub search_dirs: Vec<PathBuf>,
    /// Standard library location specified by the user, overriding discovery
    pub stdlib_dir: Option<PathBuf>,
}

impl From<&Cli> for ServerConfig {
    fn from(cli: &Cli) -> Self {
        Self {
            stdlib_dir: cli
                .stdlib_dir
                .as_ref()
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
            ..Self::new(cli.client, cli.search_dirs.clone())
        }
    }
}

impl ServerConfig {
    pub fn new(lsp_client: Option<LspClient>, search_dirs: Vec<String>) -> Self {
        // Resolve adl package dependencies in the search dirs
        Self {
            // Search dirs should already be resolved to paths (e.g. adl-vscode already resolved ${workspaceFolder} etc.)
            search_dirs: search_dirs.into_iter().map(PathBuf::from).collect(),
            stdlib_dir: None,
            _lsp_client: lsp_client,
        }
    }
}
