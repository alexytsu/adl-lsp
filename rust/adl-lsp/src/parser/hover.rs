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
    fn get_hover_text(&self, nid: usize, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let mut results = vec![];
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        Self::advance_cursor_to(&mut cursor, nid);

        debug!("advanced to node: {:?}", cursor.node());

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

        debug!("found node: {:?}", def_node);

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

    // FIXME: hover text parsing is a mess given the current tree-sitter-adl grammar since doccomments are part of the definition node
    fn _get_hover_text(&self, nid: usize, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        Self::advance_cursor_to(&mut cursor, nid);

        debug!("advanced to node: {:?}", cursor.node());

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

        debug!("found node: {:?}", def_node);

        // find the definition text including doccomments
        // let def_text = def_node
        //     .utf8_text(content.as_ref())
        //     .ok()
        //     .map(|s| s.trim().to_string());

        // collect the doccomments preceding the definition
        let def_text = self.collect_definition_text(&mut cursor.clone(), content.as_ref());
        debug!("found definition text: {:?}", def_text);
        let doc_comments = self.collect_docstrings(&mut cursor, content.as_ref());
        debug!("found doccomments: {:?}", doc_comments);

        // TODO: collect annotations defined above the definition
        // TODO: collect annotations from separate definitions

        let mut hover_text = vec![];

        if !doc_comments.is_empty() {
            hover_text.push(MarkedString::String(doc_comments.join("\n")));
        } else {
            debug!("no doccomments found for {:?}", def_node);
        }

        if !def_text.is_empty() {
            hover_text.push(MarkedString::LanguageString(lsp_types::LanguageString {
                language: "adl".into(),
                value: def_text.join("\n"),
            }));
        } else {
            error!("no definition text found for {:?}", def_node);
        }

        hover_text
    }

    /// Collects preceding comments and docstrings
    fn collect_docstrings(
        &self,
        cursor: &mut tree_sitter::TreeCursor,
        content: &[u8],
    ) -> Vec<String> {
        // FIXME: the tree-sitter-adl grammar should be changed to support interleaved comments and docstrings
        // perhaps docstrings should be a sibling of the definition node rather than a child
        let mut comments = Vec::new();

        if !cursor.goto_first_child() {
            return comments;
        };

        let mut node = cursor.node();
        // seek past docstrings and comments but don't collect comments
        while NodeKind::is_docstring(&node) || NodeKind::is_comment(&node) {
            if NodeKind::is_comment(&node) {
                continue;
            }

            if let Ok(text) = node.utf8_text(content) {
                debug!("found docstring: {:?}", text);
                comments.push(text.trim().trim_start_matches("///").trim().to_string());
            } else {
                error!("no comment found for {:?}", node);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
            node = cursor.node();
        }

        comments
    }

    /// Collects preceding comments and docstrings
    fn collect_definition_text(
        &self,
        cursor: &mut tree_sitter::TreeCursor,
        content: &[u8],
    ) -> Vec<String> {
        // FIXME: the tree-sitter-adl grammar should be changed to support interleaved comments and docstrings
        // perhaps docstrings should be a sibling of the definition node rather than a child
        let mut definition_text = Vec::new();

        if !cursor.goto_first_child() {
            return definition_text;
        };

        loop {
            let node = cursor.node();
            if !(NodeKind::is_docstring(&node) || NodeKind::is_comment(&node)) {
                if let Ok(text) = node.utf8_text(content) {
                    debug!("found definition text: {:?}", text);
                    definition_text.push(text.trim().trim_start_matches("///").trim().to_string());
                } else {
                    error!("no comment found for {:?}", node);
                }
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }

        definition_text
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
