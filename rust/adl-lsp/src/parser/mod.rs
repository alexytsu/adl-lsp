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
pub mod primitives;
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

/// Conformance: the parser must handle the canonical ADL corpus (adl-lang/adl stdlib and
/// compiler test inputs) without a single ERROR or MISSING node. The corpus is vendored by
/// tree-sitter-adl (Workstream A) at `test/canonical/`, reached via the path dependency.
#[cfg(test)]
mod conformance {
    use super::AdlParser;
    use crate::node::NodeKind;
    use crate::parser::tree::Tree;
    use lsp_types::Url;
    use std::path::{Path, PathBuf};

    fn collect_adl_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_adl_files(&path, files);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".adl") || n.contains(".adl-"))
            {
                files.push(path);
            }
        }
    }

    #[test]
    fn canonical_corpus_parses_without_errors() {
        let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tree-sitter-adl/test/canonical");
        assert!(
            corpus.exists(),
            "canonical corpus not found at {} — requires the tree-sitter-adl path dependency \
             checkout with its vendored corpus",
            corpus.display()
        );

        let mut files = Vec::new();
        collect_adl_files(&corpus, &mut files);
        assert!(!files.is_empty(), "no .adl files found in corpus");

        let mut parser = AdlParser::new();
        let mut failures = Vec::new();
        for path in &files {
            let contents = match std::fs::read_to_string(path) {
                Ok(contents) => contents,
                Err(_) => continue, // non-utf8 corpus entries are out of scope
            };
            let uri = Url::from_file_path(path).expect("absolute path");
            let Some(tree) = parser.parse(uri, contents.as_bytes()) else {
                failures.push(format!("{}: parser returned no tree", path.display()));
                continue;
            };
            let errors = tree.find_all_nodes(NodeKind::is_error).len();
            let missing = tree.find_all_nodes(NodeKind::is_missing).len();
            if errors > 0 || missing > 0 {
                failures.push(format!(
                    "{}: {errors} ERROR node(s), {missing} MISSING node(s)",
                    path.display()
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "{} of {} corpus files failed to parse cleanly:\n{}",
            failures.len(),
            files.len(),
            failures.join("\n")
        );
    }
}

impl ParsedTree {
    pub fn find_module_definition(&self) -> Option<AdlModuleDefinition> {
        self.find_first_node(NodeKind::is_module_definition)
            .and_then(AdlModuleDefinition::try_new)
    }

    pub fn find_module_name<'c>(&self, content: &'c [u8]) -> Option<&'c str> {
        let module_body_node = self.find_first_node(NodeKind::is_module_definition)?;
        let module_body_node = AdlModuleDefinition::try_new(module_body_node)?;
        Some(module_body_node.module_name(content))
    }
}
