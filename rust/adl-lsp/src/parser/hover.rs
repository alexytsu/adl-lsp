use lsp_types::MarkedString;
use tracing::{debug, error};
use tree_sitter::Node;

use crate::node::NodeKind;
use crate::parser::ParsedTree;
use crate::parser::tree::Tree;

pub trait Hover {
    fn hover(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<MarkedString>;
}

impl Hover for ParsedTree {
    fn hover(&self, identifier: &str, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let mut results = vec![];
        self.hover_impl(identifier, self.tree.root_node(), &mut results, content);
        results
    }
}

impl ParsedTree {
    fn hover_impl(
        &self,
        identifier: &str,
        n: Node,
        v: &mut Vec<MarkedString>,
        content: impl AsRef<[u8]>,
    ) {
        if identifier.is_empty() {
            return;
        }

        let hoverable_nodes: Vec<Node> = self
            .find_all_nodes_from(n, NodeKind::is_user_defined_name)
            .into_iter()
            .filter(|n| {
                let identifier_node = n.child(0);
                if let Some(identifier_node) = identifier_node {
                    identifier_node
                        .utf8_text(content.as_ref())
                        .expect("utf-8 parse error")
                        == identifier
                } else {
                    false
                }
            })
            .collect();

        debug!("Found hoverable nodes {:?}", hoverable_nodes);

        hoverable_nodes.iter().for_each(|n| {
            let hover_text = self.get_hover_text(n.id(), content.as_ref());
            v.extend(hover_text);
        });
    }

    /**
     * Finds the preceding comments and docstrings for a given node and the text of the node itself
     */
    fn get_hover_text(&self, nid: usize, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        Self::advance_cursor_to(&mut cursor, nid);

        debug!("Advanced to node: {:?}", cursor.node());

        // Cursor is now advanced to a user_defined_name
        if !NodeKind::is_user_defined_name(&cursor.node()) {
            error!("cursor is not on a user_defined_name");
            return vec![];
        }

        if !cursor.goto_parent() {
            return vec![];
        }

        debug!("Found node: {:?}", cursor.node());
        // Cursor is now advanced from name -> definition or field
        let definition = cursor.node();
        let definition_text = definition
            .utf8_text(content.as_ref())
            .expect("utf-8 parser error")
            .trim();

        cursor.goto_previous_sibling();

        let mut comments = vec![];
        while NodeKind::is_comment(&cursor.node()) || NodeKind::is_docstring(&cursor.node()) {
            let node = cursor.node();
            let text = node
                .utf8_text(content.as_ref())
                .expect("utf-8 parser error")
                .trim()
                .trim_start_matches("///")
                .trim_start_matches("//")
                .trim();

            comments.push(text);

            if !cursor.goto_previous_sibling() {
                break;
            }
        }

        let mut hover_text = vec![];
        if !comments.is_empty() {
            comments.reverse();
            hover_text.push(MarkedString::String(comments.join("\n")));
        }
        hover_text.push(MarkedString::LanguageString(lsp_types::LanguageString {
            language: "adl".into(),
            value: definition_text.into(),
        }));

        hover_text
    }
}

#[cfg(test)]
mod test {
    use async_lsp::lsp_types::Url;
    use insta::assert_yaml_snapshot;

    use crate::parser::{AdlParser, hover::Hover};

    #[test]
    fn test_hover() {
        let uri: Url = "file://input/hover.adl".parse().unwrap();
        let contents = include_str!("input/hover.adl");

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        let message = tree.hover("Message", contents.as_bytes());
        assert_yaml_snapshot!(message);

        let title = tree.hover("title", contents.as_bytes());
        assert_yaml_snapshot!(title);

        let body = tree.hover("body", contents.as_bytes());
        assert_yaml_snapshot!(body);
    }
}