use lsp_types::MarkedString;
use tracing::error;
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

        self.find_all_nodes_from(n, NodeKind::is_user_defined_name)
            .into_iter()
            .filter_map(|n| {
                n.child(0)
                    .filter(|id_node| Self::is_from_definition(id_node))
                    .and_then(|id_node| id_node.utf8_text(content.as_ref()).ok())
                    .filter(|text| *text == identifier)
                    .map(|_| n)
            })
            .for_each(|n| v.extend(self.get_hover_text(n.id(), content.as_ref())));
    }

    // HACK: since the doccomments are themselves valid ADL and are part of the definition node,
    // we can just return the entire definition node as the hover text with LanguageString for sensible highlighting
    // TODO: we should return the doccomments as a MarkedString::String and the definition text as a MarkedString::LanguageString
    fn get_hover_text(&self, nid: usize, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let mut results = vec![];
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        Self::advance_cursor_to(&mut cursor, nid);

        let node = cursor.node();
        if !NodeKind::is_user_defined_name(&node) {
            error!(
                "cursor is not on a user_defined_name: {:?} {:?}",
                node,
                node.utf8_text(content.as_ref()).ok()
            );
            return vec![];
        }

        let Some(def_node) = cursor.goto_parent().then_some(cursor.node()) else {
            return vec![];
        };

        // find the definition text including doccomments
        let def_text = def_node
            .utf8_text(content.as_ref())
            .ok()
            .map(|s| s.trim().to_string());

        if let Some(def_text) = def_text {
            results.push(MarkedString::LanguageString(lsp_types::LanguageString {
                language: "adl".into(),
                value: def_text,
            }));
        }

        results
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
