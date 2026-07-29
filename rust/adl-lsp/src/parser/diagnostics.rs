use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};
use std::collections::HashSet;
use tracing::debug;
use tree_sitter::Node;

use crate::node::{AdlImportDeclaration, AdlModuleBody, NodeKind};
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop::ts_to_lsp_position;

use super::ParsedTree;

impl ParsedTree {
    pub fn collect_diagnostics(&self, content: &str) -> Vec<Diagnostic> {
        if content.trim().is_empty() {
            return vec![Diagnostic {
                severity: Some(DiagnosticSeverity::WARNING),
                message: "empty file".to_string(),
                ..Default::default()
            }];
        }

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        // first collect parse errors
        self.collect_parse_diagnostics(&mut diagnostics);
        self.collect_parse_diagnostics_missing(&mut diagnostics);

        // then collect custom semantic errors
        if let Some(import_diagnostics) = self.collect_import_diagnostics(content.as_bytes()) {
            diagnostics.extend(import_diagnostics);
        }
        diagnostics.extend(self.collect_missing_semicolon_diagnostics());

        debug!("collected diagnostics: {:?}", diagnostics);
        diagnostics
    }

    pub fn collect_parse_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_error)
                .into_iter()
                .map(|n| {
                    let message = match n.parent() {
                        Some(parent) => format!("syntax error in {}", parent.kind()),
                        None => "syntax error".to_string(),
                    };

                    Diagnostic {
                        range: Range {
                            start: ts_to_lsp_position(&n.start_position()),
                            end: ts_to_lsp_position(&n.end_position()),
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        message,
                        ..Default::default()
                    }
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
                    message: "missing token '".to_string() + n.kind() + "'",
                    ..Default::default()
                }),
        );
    }

    pub fn collect_missing_semicolon_diagnostics(&self) -> Vec<Diagnostic> {
        // Helper function to create diagnostic for missing semicolon
        let create_missing_semicolon_diagnostic = |n: Node| Diagnostic {
            range: Range {
                start: ts_to_lsp_position(&n.start_position()),
                end: ts_to_lsp_position(&n.end_position()),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: "missing semicolon".to_string(),
            ..Default::default()
        };

        let node_kinds: &[fn(&Node) -> bool] = &[
            NodeKind::is_module_definition,
            NodeKind::is_import_declaration,
            NodeKind::is_type_definition,
            NodeKind::is_newtype_definition,
            NodeKind::is_struct_definition,
            NodeKind::is_union_definition,
            NodeKind::is_field,
            NodeKind::is_annotation_declaration,
        ];

        node_kinds
            .iter()
            .flat_map(|predicate| {
                self.find_all_nodes(*predicate)
                    .into_iter()
                    .filter(|n| is_missing_semicolon(*n))
                    .map(create_missing_semicolon_diagnostic)
            })
            .collect()
    }

    pub fn collect_import_diagnostics(&self, content: &[u8]) -> Option<Vec<Diagnostic>> {
        let imports = self.find_all_nodes(NodeKind::is_import_declaration);

        let module_body = AdlModuleBody::try_new(self.find_first_node(NodeKind::is_module_body)?)?;
        let mut cursor = module_body.cursor();
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
        }

        let out_of_order_imports: Vec<Diagnostic> = if let Some(first_non_import) = first_non_import
        {
            imports
                .iter()
                .filter_map(|node| {
                    // imports should only be at the top of a module
                    if node.start_position() > first_non_import.start_position() {
                        Some(Diagnostic {
                            range: Range {
                                start: ts_to_lsp_position(&node.start_position()),
                                end: ts_to_lsp_position(&node.end_position()),
                            },
                            message: "imports must be declared at the beginning of a module"
                                .to_string(),
                            severity: Some(DiagnosticSeverity::ERROR),
                            ..Default::default()
                        })
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut seen_imports = HashSet::new();
        let duplicate_imports: Vec<Diagnostic> = imports
            .iter()
            .filter_map(|node| {
                let import_decl = AdlImportDeclaration::try_new(*node)?;
                let key = match import_decl {
                    AdlImportDeclaration::FullyQualified(_) => {
                        let module = import_decl.module_name(content);
                        let type_name = import_decl.imported_type_name(content).unwrap_or_default();
                        format!("{module}.{type_name}")
                    }
                    AdlImportDeclaration::StarImport(_) => {
                        format!("{}.*", import_decl.module_name(content))
                    }
                };

                if seen_imports.insert(key) {
                    None
                } else {
                    Some(Diagnostic {
                        range: Range {
                            start: ts_to_lsp_position(&node.start_position()),
                            end: ts_to_lsp_position(&node.end_position()),
                        },
                        message: "duplicate import".to_string(),
                        severity: Some(DiagnosticSeverity::WARNING),
                        ..Default::default()
                    })
                }
            })
            .collect();

        // TODO(med): attempt to resolve imports and report errors for invalid imports
        // TODO(med): check for unused or duplicate imports

        let mut diagnostics = out_of_order_imports;
        diagnostics.extend(duplicate_imports);

        Some(diagnostics)
    }
}

fn is_missing_semicolon(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let last_child = cursor.goto_last_child();
    last_child && cursor.node().kind() != ";"
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

        let parsed = AdlParser::new().parse(url.clone(), contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_diagnostics(contents));
    }

    #[test]
    fn test_collect_import_error() {
        let url: Url = "file://foo/importerror.adl".parse().unwrap();
        let contents = include_str!("input/importerror.adl");

        let parsed = AdlParser::new().parse(url.clone(), contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_diagnostics(contents));
    }

    #[test]
    fn test_collect_missing_semicolon_error() {
        let url: Url = "file://foo/missing_semicolons.adl".parse().unwrap();
        let contents = include_str!("input/missing_semicolons.adl");

        let parsed = AdlParser::new().parse(url.clone(), contents);
        assert!(parsed.is_some());
        assert_yaml_snapshot!(parsed.unwrap().collect_diagnostics(contents));
    }

    #[test]
    fn test_collect_missing_semicolon_no_error() {
        let url: Url = "file://foo/message.adl".parse().unwrap();
        let contents = include_str!("input/message.adl");

        let parsed = AdlParser::new().parse(url.clone(), contents);
        assert!(parsed.is_some());
        let diagnostics = parsed.unwrap().collect_missing_semicolon_diagnostics();
        assert!(diagnostics.is_empty());
    }
}
