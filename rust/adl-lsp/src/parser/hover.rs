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

    /// Build hover contents for a definition, splitting the leading doccomments (rendered as
    /// markdown prose) from the definition body (rendered as syntax-highlighted ADL).
    ///
    /// Doccomments are part of the definition node in the tree-sitter grammar (inside a
    /// `definition_preamble`), so we walk the definition's children, peel off the preamble
    /// docstrings/comments, and emit the remainder as code.
    fn get_hover_text(&self, nid: usize, content: impl AsRef<[u8]>) -> Vec<MarkedString> {
        let content = content.as_ref();
        let mut results = vec![];
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        Self::advance_cursor_to(&mut cursor, nid);

        let node = cursor.node();
        if !NodeKind::is_user_defined_name(&node) {
            error!(
                "cursor is not on a user_defined_name: {:?} {:?}",
                node,
                node.utf8_text(content).ok()
            );
            return vec![];
        }

        let Some(def_node) = cursor.goto_parent().then_some(cursor.node()) else {
            return vec![];
        };

        // Separate the doccomment preamble from the code portion of the definition.
        let mut doc_lines: Vec<String> = Vec::new();
        let mut code_start_byte = def_node.end_byte();
        let mut child_cursor = def_node.walk();
        for child in def_node.children(&mut child_cursor) {
            if NodeKind::is_definition_preamble(&child) {
                let mut preamble_cursor = child.walk();
                for preamble_child in child.children(&mut preamble_cursor) {
                    if NodeKind::is_docstring(&preamble_child)
                        || NodeKind::is_comment(&preamble_child)
                    {
                        if let Ok(text) = preamble_child.utf8_text(content) {
                            doc_lines.push(strip_doc_marker(text));
                        }
                    }
                }
            } else if NodeKind::is_docstring(&child) || NodeKind::is_comment(&child) {
                if let Ok(text) = child.utf8_text(content) {
                    doc_lines.push(strip_doc_marker(text));
                }
            } else {
                // First non-doc child marks where the definition body begins.
                code_start_byte = child.start_byte();
                break;
            }
        }

        if !doc_lines.is_empty() {
            results.push(MarkedString::String(doc_lines.join("\n")));
        }

        let code_text = std::str::from_utf8(&content[code_start_byte..def_node.end_byte()])
            .ok()
            .map(|s| s.trim().to_string());

        if let Some(code_text) = code_text.filter(|s| !s.is_empty()) {
            results.push(MarkedString::LanguageString(lsp_types::LanguageString {
                language: "adl".into(),
                value: code_text,
            }));
        }

        results
    }
}

/// Strip the leading `///`/`//` doc markers and surrounding whitespace from a comment line.
fn strip_doc_marker(text: &str) -> String {
    text.trim().trim_start_matches('/').trim().to_string()
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
