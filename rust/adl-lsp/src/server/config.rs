use std::path::PathBuf;

use crate::cli::{Cli, LspClient};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    _lsp_client: Option<LspClient>,
    // TODO: instead of explicitly passing package roots, use the closest adl-package.json file in the line of parent directories
    pub package_roots: Vec<PathBuf>,
}

impl From<&Cli> for ServerConfig {
    fn from(cli: &Cli) -> Self {
        Self::new(cli.client, cli.package_roots.clone())
    }
}

impl ServerConfig {
    pub fn new(lsp_client: Option<LspClient>, package_roots: Vec<String>) -> Self {
        Self {
            package_roots: Self::resolve_package_roots(lsp_client, package_roots),
            _lsp_client: lsp_client,
        }
    }

    fn resolve_package_roots(
        lsp_client: Option<LspClient>,
        package_roots: Vec<String>,
    ) -> Vec<PathBuf> {
        match lsp_client {
            Some(LspClient::VSCode) => {
                // TODO: deal with {workspaceFolder} etc.
                package_roots.into_iter().map(PathBuf::from).collect()
            }
            None => package_roots.into_iter().map(PathBuf::from).collect(),
        }
    }
}
