use async_lsp::lsp_types::{Location, Range};
use serde::{Deserialize, Serialize};
use tracing::debug;
use tree_sitter::Node;

use crate::node::NodeKind;
use crate::parser::ParsedTree;
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop;

#[derive(Debug)]
enum DefinitionKind<'a> {
    Definition(Node<'a>),
    Import(Node<'a>, String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub source_module: String,
    pub target_module_path: Vec<String>,
    pub identifier: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DefinitionLocation {
    Resolved(Location),
    Import(UnresolvedImport),
}

pub trait Definition {
    fn definition(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<DefinitionLocation>;
}

impl Definition for ParsedTree {
    fn definition(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<DefinitionLocation> {
        let mut results = vec![];
        self.definition_impl(identifier, self.tree.root_node(), &mut results, content);
        results
    }
}

impl ParsedTree {
    pub fn is_from_definition(node: &Node<'_>) -> bool {
        NodeKind::is_definition(node) || node.parent().is_some_and(|p| Self::is_from_definition(&p))
    }

    pub fn is_from_import_declaration<'a>(node: &Node<'a>) -> (bool, Option<Node<'a>>) {
        if NodeKind::is_import_declaration(node) {
            (true, Some(*node))
        } else {
            node.parent()
                .map_or((false, None), |p| Self::is_from_import_declaration(&p))
        }
    }

    fn definition_impl(
        &self,
        identifier: &str,
        n: Node,
        v: &mut Vec<DefinitionLocation>,
        content: impl AsRef<[u8]>,
    ) {
        if identifier.is_empty() {
            return;
        }

        let locations: Vec<DefinitionLocation> = self
            .find_all_nodes_from(n, NodeKind::is_user_defined_name)
            .into_iter()
            .filter(|n| n.utf8_text(content.as_ref()).expect("utf-8 parse error") == identifier)
            .filter(|n| {
                let is_from_definition = Self::is_from_definition(n);
                let is_from_import = Self::is_from_import_declaration(&n).0;
                if is_from_definition {
                    debug!(
                        "is_from_import_definition: {:?}",
                        n.utf8_text(content.as_ref()).unwrap()
                    );
                    debug!("n: {:?}", n.kind());
                }
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

        v.extend(locations);
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
                // resolve the import path to the form e.g. "common.strings.StringML"
                let import_path = import_declaration.child(1).unwrap();
                let import_path_text = import_path.utf8_text(content.as_ref()).unwrap();

                let import_module_path: Vec<String> =
                    import_path_text.split(".").map(|s| s.into()).collect();
                let import_module_path = &import_module_path[..import_module_path.len() - 1];
                debug!("Import: {:?}", import_path_text);

                DefinitionLocation::Import(UnresolvedImport {
                    // keep going up the tree to find the source module
                    source_module: Self::get_source_module(&import_declaration, &content).unwrap(),
                    target_module_path: import_module_path.to_vec(),
                    identifier, // could be common, strings or StringML
                })
            }
        }
    }

    fn get_source_module(node: &Node<'_>, content: impl AsRef<[u8]>) -> Option<String> {
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
        assert_yaml_snapshot!(message);

        let content = tree.definition("Content", contents.as_bytes());
        assert_yaml_snapshot!(content);

        let name = tree.definition("Name", contents.as_bytes());
        assert_yaml_snapshot!(name);

        let string = tree.definition("String", contents.as_bytes());
        assert_yaml_snapshot!(string);

        let user = tree.definition("User", contents.as_bytes());
        assert_yaml_snapshot!(user);
    }
}
