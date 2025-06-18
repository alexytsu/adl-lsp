use async_lsp::lsp_types::Url;
use std::sync::Arc;
use tree_sitter::Tree as TsTree;

use crate::{
    node::{AdlModuleDefinition, NodeKind},
    parser::tree::Tree,
};

pub mod definition;
pub mod diagnostics;
pub mod hover;
pub mod references;
pub mod symbols;
pub mod tree;
pub mod ts_lsp_interop;

pub struct AdlParser {
    parser: tree_sitter::Parser,
}

#[derive(Clone)]
pub struct ParsedTree {
    pub uri: Url,
    tree: Arc<TsTree>,
}

impl AdlParser {
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        if let Err(e) = parser.set_language(&tree_sitter_adl::LANGUAGE.into()) {
            panic!("failed to set ts language parser {:?}", e);
        }
        Self { parser }
    }

    pub fn parse(&mut self, uri: Url, contents: impl AsRef<[u8]>) -> Option<ParsedTree> {
        self.parser.parse(contents, None).map(|t| ParsedTree {
            tree: Arc::new(t),
            uri,
        })
    }
}

impl Default for AdlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ParsedTree {
    pub fn get_module_name<'c>(&self, content: &'c [u8]) -> Option<&'c str> {
        let module_body_node = self.find_first_node(NodeKind::is_module_definition)?;
        let module_body_node = AdlModuleDefinition::try_new(module_body_node)?;
        Some(module_body_node.module_name(content))
    }
}
