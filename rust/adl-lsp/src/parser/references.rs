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
        self.find_references_impl(identifier, self.tree.root_node(), &mut results, content);
        results
    }
}

impl ParsedTree {
    fn find_references_impl(
        &self,
        identifier: &str,
        n: Node,
        v: &mut Vec<Location>,
        content: impl AsRef<[u8]>,
    ) {
        if identifier.is_empty() {
            return;
        }

        // Find all user-defined names in the tree
        let all_user_defined_names = self.find_all_nodes_from(n, NodeKind::is_user_defined_name);

        let matching_names: Vec<_> = all_user_defined_names
            .into_iter()
            .filter(|n| n.utf8_text(content.as_ref()).expect("utf-8 parse error") == identifier)
            .collect();

        let filtered_names: Vec<_> = matching_names
            .into_iter()
            .filter(|n| {
                // Include all usages except definitions and imports
                let is_from_definition = Self::is_from_definition(n);
                let is_from_import = Self::is_from_import_declaration(n).0;
                !is_from_definition && !is_from_import
            })
            .collect();

        let deduped_names: Vec<_> = filtered_names
            .into_iter()
            .filter(|n| {
                // Prefer scoped_name over identifier when they have the same position
                // This avoids duplicates from scoped_name containing identifier
                // TODO(med): investigate this further. is this is a hack? i think we can probably just ignore identifiers
                let should_include = if NodeKind::is_scoped_name(n) {
                    true
                } else if NodeKind::is_identifier(n) {
                    // Only include identifier if it's not a direct child of scoped_name with same text
                    if let Some(parent) = n.parent() {
                        let is_child_of_scoped_name = NodeKind::is_scoped_name(&parent)
                            && parent.utf8_text(content.as_ref()).unwrap_or("") == identifier;
                        !is_child_of_scoped_name
                    } else {
                        true
                    }
                } else {
                    true
                };

                should_include
            })
            .collect();

        let locations: Vec<Location> = deduped_names
            .into_iter()
            .map(|n| Location {
                uri: self.uri.clone(),
                range: Range {
                    start: ts_lsp_interop::ts_to_lsp_position(&n.start_position()),
                    end: ts_lsp_interop::ts_to_lsp_position(&n.end_position()),
                },
            })
            .collect();

        v.extend(locations);
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
