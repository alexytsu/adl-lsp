use async_lsp::lsp_types::{Location, Range};
use tree_sitter::Node;

use crate::node::NodeKind;
use crate::parser::ParsedTree;
use crate::parser::tree::Tree;
use crate::parser::ts_lsp_interop;

pub trait References {
    fn find_references(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<Location>;
}

impl References for ParsedTree {
    fn find_references(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<Location> {
        let mut results = vec![];
        self.find_references_impl(
            identifier,
            self.tree.root_node(),
            &mut results,
            content.as_ref(),
        );
        results
    }
}

impl ParsedTree {
    fn find_references_impl(
        &self,
        identifier: &str,
        n: Node,
        v: &mut Vec<Location>,
        content: &[u8],
    ) {
        if identifier.is_empty() {
            return;
        }

        let locations = self
            .find_all_nodes_from(n, NodeKind::is_user_defined_name)
            .into_iter()
            .filter_map(|node| {
                let text = node.utf8_text(content).ok()?;
                if text != identifier {
                    return None;
                }

                // Include all usages except definitions and imports
                if Self::is_from_definition(&node) || Self::find_import_declaration(&node).is_some()
                {
                    return None;
                }

                // Prefer scoped_name over identifier when they have the same position
                if !Self::should_include_reference(&node, identifier, content) {
                    return None;
                }

                Some(Location {
                    uri: self.uri.clone(),
                    range: Range {
                        start: ts_lsp_interop::ts_to_lsp_position(&node.start_position()),
                        end: ts_lsp_interop::ts_to_lsp_position(&node.end_position()),
                    },
                })
            })
            .collect::<Vec<_>>();

        v.extend(locations);
    }

    fn should_include_reference(node: &Node<'_>, identifier: &str, content: &[u8]) -> bool {
        // TODO(med): investigate this further. is this is a hack? i think we can probably just ignore identifiers
        if NodeKind::is_scoped_name(node) {
            return true;
        }

        if NodeKind::is_identifier(node) {
            if let Some(parent) = node.parent() {
                let is_child_of_scoped_name = NodeKind::is_scoped_name(&parent)
                    && parent.utf8_text(content).unwrap_or("") == identifier;
                return !is_child_of_scoped_name;
            }
        }

        true
    }
}

#[cfg(test)]
mod test {
    use async_lsp::lsp_types::Url;
    use insta::assert_yaml_snapshot;

    use crate::parser::{AdlParser, references::References};

    #[test]
    fn test_references() {
        let uri: Url = "file://input/message.adl".parse().unwrap();
        let contents = include_str!("input/message.adl");

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Test that Message, String have no references in this file
        // (they are only defined or imported, not used)
        let message_refs = tree.find_references("Message", contents.as_bytes());
        assert_yaml_snapshot!(message_refs);

        let string_refs = tree.find_references("String", contents.as_bytes());
        assert_yaml_snapshot!(string_refs);

        // Test that Content, Name, User have references (they are used in struct fields)
        let content_refs = tree.find_references("Content", contents.as_bytes());
        assert_yaml_snapshot!(content_refs);

        let name_refs = tree.find_references("Name", contents.as_bytes());
        assert_yaml_snapshot!(name_refs);

        let user_refs = tree.find_references("User", contents.as_bytes());
        assert_yaml_snapshot!(user_refs);
    }
}
