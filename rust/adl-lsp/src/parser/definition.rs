use async_lsp::lsp_types::{Location, Range};
use serde::{Deserialize, Serialize};
use tracing::debug;
use tree_sitter::Node;

use crate::node::{AdlImportDeclaration, NodeKind};
use crate::parser::ParsedTree;
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop;

#[derive(Clone, Debug)]
enum DefinitionKind<'a> {
    Definition(Node<'a>),
    Import(AdlImportDeclaration<'a>, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub source_module: String,
    pub target_module_path: Vec<String>,
    pub identifier: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DefinitionLocation {
    Resolved(Location),
    Import(UnresolvedImport),
}

pub trait Definition {
    fn definition(&self, identifier: &str, content: impl AsRef<[u8]>)
    -> Option<DefinitionLocation>;
}

impl Definition for ParsedTree {
    fn definition(
        &self,
        identifier: &str,
        content: impl AsRef<[u8]>,
    ) -> Option<DefinitionLocation> {
        self.definition_impl(identifier, self.tree.root_node(), content)
    }
}

// TODO: handle goto field for annotations
impl ParsedTree {
    pub fn is_from_definition(node: &Node<'_>) -> bool {
        NodeKind::is_definition(node) || node.parent().is_some_and(|p| Self::is_from_definition(&p))
    }

    pub fn is_from_import_declaration<'a>(
        node: &Node<'a>,
    ) -> (bool, Option<AdlImportDeclaration<'a>>) {
        if NodeKind::is_import_declaration(node) {
            (
                true,
                Some(AdlImportDeclaration::try_new(*node).expect("expected import_declaration")),
            )
        } else {
            node.parent()
                .map_or((false, None), |p| Self::is_from_import_declaration(&p))
        }
    }

    fn definition_impl(
        &self,
        identifier: &str,
        n: Node,
        content: impl AsRef<[u8]>,
    ) -> Option<DefinitionLocation> {
        if identifier.is_empty() {
            return None;
        }

        let mut locations: Vec<DefinitionLocation> = self
            .find_all_nodes_from(n, NodeKind::is_user_defined_name)
            .into_iter()
            .filter(|n| n.utf8_text(content.as_ref()).expect("utf-8 parse error") == identifier)
            .filter(|n| {
                let is_from_definition = Self::is_from_definition(n);
                let is_from_import = Self::is_from_import_declaration(n).0;
                is_from_import || (is_from_definition && !NodeKind::is_identifier(n))
            })
            .map(|n| {
                if let (true, Some(import_node)) = Self::is_from_import_declaration(&n) {
                    DefinitionKind::Import(import_node, identifier.into())
                } else {
                    DefinitionKind::Definition(n)
                }
            })
            .map(|n| self.definition_location(n, &content))
            .collect();

        // If no direct matches found, look for the identifier as part of scoped names
        if locations.is_empty() {
            locations = self
                .find_all_nodes_from(n, NodeKind::is_scoped_name)
                .into_iter()
                .filter(|scoped_node| {
                    let scoped_text = scoped_node
                        .utf8_text(content.as_ref())
                        .expect("utf-8 parse error");
                    // Check if the scoped name ends with our identifier (e.g., "common.string.StringNE" ends with "StringNE")
                    scoped_text.ends_with(&format!(".{}", identifier)) || scoped_text == identifier
                })
                .map(|scoped_node| {
                    // For scoped names, we treat them as imports that need to be resolved
                    let scoped_text = scoped_node
                        .utf8_text(content.as_ref())
                        .expect("utf-8 parse error");
                    let parts: Vec<&str> = scoped_text.split('.').collect();
                    if parts.len() > 1 {
                        // Create an unresolved import for the scoped name
                        DefinitionLocation::Import(UnresolvedImport {
                            source_module: Self::get_source_module(&scoped_node, &content)
                                .unwrap_or_default(),
                            target_module_path: parts[..parts.len() - 1]
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                            identifier: identifier.to_string(),
                        })
                    } else {
                        // Single identifier, treat as local definition
                        DefinitionLocation::Resolved(Location {
                            uri: self.uri.clone(),
                            range: Range {
                                start: ts_lsp_interop::ts_to_lsp_position(
                                    &scoped_node.start_position(),
                                ),
                                end: ts_lsp_interop::ts_to_lsp_position(
                                    &scoped_node.end_position(),
                                ),
                            },
                        })
                    }
                })
                .collect();
        }

        locations.first().cloned()
    }

    fn definition_location(
        &self,
        d: DefinitionKind,
        content: impl AsRef<[u8]>,
    ) -> DefinitionLocation {
        match d {
            DefinitionKind::Definition(n) => {
                debug!("Definition: {:?}", n.utf8_text(content.as_ref()).unwrap());
                DefinitionLocation::Resolved(Location {
                    uri: self.uri.clone(),
                    range: Range {
                        start: ts_lsp_interop::ts_to_lsp_position(&n.start_position()),
                        end: ts_lsp_interop::ts_to_lsp_position(&n.end_position()),
                    },
                })
            }
            DefinitionKind::Import(import_declaration, identifier) => {
                DefinitionLocation::Import(UnresolvedImport {
                    source_module: self
                        .get_module_name(content.as_ref())
                        .expect("expected module name")
                        .to_string(),
                    target_module_path: import_declaration
                        .module_name(content.as_ref())
                        .split(".")
                        .map(|s| s.to_string())
                        .collect(),
                    identifier, // could be common, strings or StringML
                })
            }
        }
    }

    pub fn get_source_module(node: &Node<'_>, content: impl AsRef<[u8]>) -> Option<String> {
        if NodeKind::is_module_definition(node) {
            return node
                .child(1)
                .and_then(|child| child.utf8_text(content.as_ref()).ok())
                .map(String::from);
        }
        node.parent()
            .and_then(|p| Self::get_source_module(&p, content))
    }
}

#[cfg(test)]
mod test {
    use async_lsp::lsp_types::Url;
    use insta::assert_yaml_snapshot;

    use crate::parser::{AdlParser, definition::Definition};

    #[test]
    fn test_definition() {
        let uri: Url = "file://input/message.adl".parse().unwrap();
        let contents = include_str!("input/message.adl");

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        let message = tree.definition("Message", contents.as_bytes());
        assert_yaml_snapshot!("Message", message);

        let content = tree.definition("Content", contents.as_bytes());
        assert_yaml_snapshot!("Content", content);

        let name = tree.definition("Name", contents.as_bytes());
        assert_yaml_snapshot!("Name", name);

        let string = tree.definition("String", contents.as_bytes());
        assert_yaml_snapshot!("String", string);

        let user = tree.definition("User", contents.as_bytes());
        assert_yaml_snapshot!("User", user);

        let string_not_empty = tree.definition("StringNE", contents.as_bytes());
        assert_yaml_snapshot!("common.string.StringNE", string_not_empty);
    }
}
