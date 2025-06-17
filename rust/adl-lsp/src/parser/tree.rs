use async_lsp::lsp_types::Position;
use tree_sitter::{Node, TreeCursor};

use crate::parser::ts_lsp_interop as ts_lsp;
use crate::{node::NodeKind, parser::ParsedTree};

/// Basic tree traversal methods for working with the tree-sitter parsed tree
pub trait Tree {
    fn advance_cursor_to(cursor: &mut TreeCursor<'_>, nid: usize) -> bool;
    fn find_all_nodes_from<'a>(&self, n: Node<'a>, f: fn(&Node) -> bool) -> Vec<Node<'a>>;
    fn walk_and_filter<'a>(
        cursor: &mut TreeCursor<'a>,
        f: fn(&Node) -> bool,
        early: bool,
    ) -> Vec<Node<'a>>;
    fn get_node_at_position<'a>(&'a self, pos: &Position) -> Option<Node<'a>>;
    fn find_all_nodes(&self, f: fn(&Node) -> bool) -> Vec<Node>;
    fn find_first_node<'a>(&'a self, f: fn(&Node) -> bool) -> Option<Node<'a>>;
    fn find_node_from<'a>(&self, n: Node<'a>, f: fn(&Node) -> bool) -> Vec<Node<'a>>;
}

impl Tree for ParsedTree {
    fn walk_and_filter<'a>(
        cursor: &mut TreeCursor<'a>,
        f: fn(&Node) -> bool,
        early: bool,
    ) -> Vec<Node<'a>> {
        let mut v = vec![];

        loop {
            let node = cursor.node();

            if f(&node) {
                v.push(node);
                if early {
                    break;
                }
            }

            if cursor.goto_first_child() {
                v.extend(Self::walk_and_filter(cursor, f, early));
                cursor.goto_parent();
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }

        v
    }

    fn advance_cursor_to(cursor: &mut TreeCursor<'_>, nid: usize) -> bool {
        loop {
            let node = cursor.node();
            if node.id() == nid {
                return true;
            }
            if cursor.goto_first_child() {
                if Self::advance_cursor_to(cursor, nid) {
                    return true;
                }
                cursor.goto_parent();
            }
            if !cursor.goto_next_sibling() {
                return false;
            }
        }
    }

    fn get_node_at_position<'a>(&'a self, pos: &Position) -> Option<Node<'a>> {
        let pos = ts_lsp::lsp_to_ts_point(pos);
        self.tree.root_node().descendant_for_point_range(pos, pos)
    }

    fn find_all_nodes(&self, f: fn(&Node) -> bool) -> Vec<Node> {
        self.find_all_nodes_from(self.tree.root_node(), f)
    }

    fn find_all_nodes_from<'a>(&self, n: Node<'a>, f: fn(&Node) -> bool) -> Vec<Node<'a>> {
        let mut cursor = n.walk();
        Self::walk_and_filter(&mut cursor, f, false)
    }

    fn find_first_node<'a>(&'a self, f: fn(&Node) -> bool) -> Option<Node<'a>> {
        self.find_node_from(self.tree.root_node(), f).first().copied()
    }

    fn find_node_from<'a>(&self, n: Node<'a>, f: fn(&Node) -> bool) -> Vec<Node<'a>> {
        let mut cursor = n.walk();
        Self::walk_and_filter(&mut cursor, f, true)
    }
}

impl ParsedTree {
    pub fn get_identifier_at<'a>(
        &'a self,
        pos: &Position,
        content: &'a [u8],
    ) -> Option<(&'a str, Node<'a>)> {
        self.get_node_at_position(pos)
            .filter(NodeKind::is_identifier)
            .map(|n| (n.utf8_text(content.as_ref()).expect("utf-8 parse error"), n))
    }
}
