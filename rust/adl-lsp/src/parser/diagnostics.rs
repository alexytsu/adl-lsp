use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};
use tracing::debug;

use crate::node::NodeKind;
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop::ts_to_lsp_position;

use super::ParsedTree;

impl ParsedTree {
    pub fn collect_diagnostics(&self, content: &str) -> Vec<Diagnostic> {
        if content.trim().is_empty() {
            return vec![Diagnostic {
                severity: Some(DiagnosticSeverity::WARNING),
                message: "Empty file".to_string(),
                ..Default::default()
            }];
        }

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        // first collect parse errors
        self.collect_parse_diagnostics(&mut diagnostics);
        self.collect_parse_diagnostics_missing(&mut diagnostics);

        // then collect custom semantic errors
        if let Some(import_diagnostics) = self.collect_import_diagnostics() {
            diagnostics.extend(import_diagnostics);
        }

        debug!("collected diagnostics: {:?}", diagnostics);
        diagnostics
    }

    pub fn collect_parse_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_error)
                .into_iter()
                .map(|n| Diagnostic {
                    range: Range {
                        start: ts_to_lsp_position(&n.start_position()),
                        end: ts_to_lsp_position(&n.end_position()),
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "Syntax error".to_string(),
                    ..Default::default()
                }),
        );
    }

    pub fn collect_parse_diagnostics_missing(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_missing)
                .into_iter()
                .map(|n| Diagnostic {
                    range: Range {
                        start: ts_to_lsp_position(&n.start_position()),
                        end: ts_to_lsp_position(&n.end_position()),
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "Missing token '".to_string() + n.kind() + "'",
                    ..Default::default()
                }),
        );
    }

    pub fn collect_import_diagnostics(&self) -> Option<Vec<Diagnostic>> {
        let imports = self.find_all_nodes(NodeKind::is_import_declaration);
        let module_body_children = self.find_first_node(NodeKind::is_module_body)?;
        let mut cursor = module_body_children.walk();
        cursor.goto_first_child(); // opening module brace

        let mut first_non_import = None;
        while cursor.goto_next_sibling() {
            let node = cursor.node();
            if !NodeKind::is_import_declaration(&node)
                && !NodeKind::is_docstring(&node)
                && !NodeKind::is_comment(&node)
            {
                first_non_import = Some(node);
                break;
            }
            cursor.goto_next_sibling();
        }

        let first_non_import = first_non_import?;

        debug!("first_non_import: {:?}", first_non_import.kind());

        // TODO: attempt to resolve the imports and report errors for invalid imports

        let diagnostics = imports
            .iter()
            .filter_map(|node| {
                // imports should only be at the top of a module
                if node.start_position() > first_non_import.start_position() {
                    Some(Diagnostic {
                        range: Range {
                            start: ts_to_lsp_position(&node.start_position()),
                            end: ts_to_lsp_position(&node.end_position()),
                        },
                        message: "Imports must be at the top of a module".to_string(),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        code_description: None,
                        source: Some("adl-lsp".to_string()),
                        related_information: None,
                        tags: None,
                        data: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        Some(diagnostics)
    }
}

#[cfg(test)]
mod test {
    use async_lsp::lsp_types::Url;
    use insta::assert_yaml_snapshot;

    use crate::parser::AdlParser;

    #[test]
    fn test_collect_parse_error() {
        let url: Url = "file://foo/error.adl".parse().unwrap();
        let contents = include_str!("input/error.adl");

        let parsed = AdlParser::new().parse(url.clone(), &contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_diagnostics(contents));
    }

    #[test]
    fn test_collect_import_error() {
        let url: Url = "file://foo/importerror.adl".parse().unwrap();
        let contents = include_str!("input/importerror.adl");

        let parsed = AdlParser::new().parse(url.clone(), &contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_diagnostics(&contents));
    }
}
