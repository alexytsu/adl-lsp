use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};
use tracing::debug;

use crate::node::{
    AdlAnnotationDeclaration, AdlField, AdlImportDeclaration, AdlModuleBody, AdlModuleDefinition,
    AdlNewtypeDefinition, AdlStructDefinition, AdlTypeDefinition, AdlUnionDefinition, NodeKind,
};
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
        if let Some(import_diagnostics) = self.collect_import_diagnostics() {
            diagnostics.extend(import_diagnostics);
        }
        if let Some(missing_semicolon_diagnostics) = self.collect_missing_semicolon_diagnostics() {
            diagnostics.extend(missing_semicolon_diagnostics);
        }

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

    pub fn collect_missing_semicolon_diagnostics(&self) -> Option<Vec<Diagnostic>> {
        // TODO(alex): abstract this over all nodes with potentially-missing semicolon
        let mut diagnostics = Vec::new();

        // Helper function to create diagnostic for missing semicolon
        let create_missing_semicolon_diagnostic = |n: tree_sitter::Node| Diagnostic {
            range: Range {
                start: ts_to_lsp_position(&n.start_position()),
                end: ts_to_lsp_position(&n.end_position()),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: "missing semicolon".to_string(),
            ..Default::default()
        };

        // Check module definitions
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_module_definition)
                .into_iter()
                .filter(|n| {
                    AdlModuleDefinition::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check import declarations
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_import_declaration)
                .into_iter()
                .filter(|n| {
                    AdlImportDeclaration::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check type definitions
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_type_definition)
                .into_iter()
                .filter(|n| {
                    AdlTypeDefinition::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check newtype definitions
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_newtype_definition)
                .into_iter()
                .filter(|n| {
                    AdlNewtypeDefinition::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check struct definitions
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_struct_definition)
                .into_iter()
                .filter(|n| {
                    AdlStructDefinition::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check union definitions
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_union_definition)
                .into_iter()
                .filter(|n| {
                    AdlUnionDefinition::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check fields
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_field)
                .into_iter()
                .filter(|n| {
                    AdlField::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        // Check annotation declarations
        diagnostics.extend(
            self.find_all_nodes(NodeKind::is_annotation_declaration)
                .into_iter()
                .filter(|n| {
                    AdlAnnotationDeclaration::try_new(*n)
                        .map(|t| t.is_missing_semicolon())
                        .unwrap_or(false)
                })
                .map(create_missing_semicolon_diagnostic),
        );

        Some(diagnostics)
    }

    pub fn collect_import_diagnostics(&self) -> Option<Vec<Diagnostic>> {
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

        let first_non_import = first_non_import?;

        let out_of_order_imports = imports
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
            .collect();

        // TODO: attempt to resolve the imports and report errors for invalid imports

        Some(out_of_order_imports)
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
}
