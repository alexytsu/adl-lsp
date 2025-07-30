use std::path::PathBuf;

use crate::cli::{Cli, LspClient};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    _lsp_client: Option<LspClient>,
    /// Search dirs for adl packages specified by the user - does not include dependencies resolved from adl-package.json
    pub search_dirs: Vec<PathBuf>,
}

impl From<&Cli> for ServerConfig {
    fn from(cli: &Cli) -> Self {
        Self::new(cli.client, cli.search_dirs.clone())
    }
}

impl ServerConfig {
    pub fn new(lsp_client: Option<LspClient>, search_dirs: Vec<String>) -> Self {
        // Resolve adl package dependencies in the search dirs
        Self {
            // Search dirs should already be resolved to paths (e.g. adl-vscode already resolved ${workspaceFolder} etc.)
            search_dirs: search_dirs.into_iter().map(PathBuf::from).collect(),
            _lsp_client: lsp_client,
        }
    }
}
