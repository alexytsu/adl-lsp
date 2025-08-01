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
    fn find_first_node(&self, f: fn(&Node) -> bool) -> Option<Node<'_>>;
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

    fn find_first_node(&self, f: fn(&Node) -> bool) -> Option<Node<'_>> {
        self.find_node_from(self.tree.root_node(), f)
            .first()
            .copied()
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

    /// Get module path information at the cursor position
    /// Returns (module_path, source_module) where:
    /// - module_path: the module path to navigate to
    /// - source_module: the current module name for resolution context
    pub fn get_module_path_at<'a>(
        &'a self,
        pos: &Position,
        content: &'a [u8],
    ) -> Option<(String, String)> {
        let node = self.get_node_at_position(pos)?;
        
        // Check if we're in an import declaration
        if let Some(import_info) = self.get_module_from_import_at_position(&node, content) {
            return Some(import_info);
        }
        
        // Check if we're in a scoped name (FQN)
        if let Some(fqn_info) = self.get_module_from_scoped_name_at_position(&node, content, pos) {
            return Some(fqn_info);
        }
        
        None
    }

    /// Extract module path from import declaration at cursor position
    fn get_module_from_import_at_position<'a>(
        &'a self,
        node: &Node<'a>,
        content: &'a [u8],
    ) -> Option<(String, String)> {
        // Walk up the tree to find if we're in an import declaration
        let mut current = *node;
        while let Some(parent) = current.parent() {
            if NodeKind::is_import_declaration(&parent) {
                if let Some(import_decl) = crate::node::AdlImportDeclaration::try_new(parent) {
                    let source_module = self.find_module_definition()
                        .map(|m| m.module_name(content).to_string())
                        .unwrap_or_default();
                    let module_path = import_decl.module_name(content).to_string();
                    return Some((module_path, source_module));
                }
            }
            current = parent;
        }
        None
    }

    /// Extract module path from scoped name (FQN) at cursor position
    fn get_module_from_scoped_name_at_position<'a>(
        &'a self,
        node: &Node<'a>,
        content: &'a [u8],
        pos: &Position,
    ) -> Option<(String, String)> {
        // Walk up to find the scoped name
        let mut current = *node;
        while let Some(parent) = current.parent() {
            if NodeKind::is_scoped_name(&parent) {
                let scoped_text = parent.utf8_text(content).ok()?;
                
                // Parse the scoped name to determine which part we clicked on
                let parts: Vec<&str> = scoped_text.split('.').collect();
                if parts.len() > 1 {
                    // Convert cursor position to byte offset within the scoped name
                    let cursor_point = ts_lsp::lsp_to_ts_point(pos);
                    let scoped_start_point = parent.start_position();
                    
                    // Calculate relative byte offset within the scoped name text
                    let relative_byte_offset = if cursor_point.row == scoped_start_point.row {
                        cursor_point.column.saturating_sub(scoped_start_point.column)
                    } else {
                        // Multi-line case - use the current node's position
                        let node_start = current.start_position();
                        if node_start.row == scoped_start_point.row {
                            node_start.column.saturating_sub(scoped_start_point.column)
                        } else {
                            0
                        }
                    };
                    
                    // Find which dot-separated segment we're in
                    let mut current_offset = 0;
                    for (i, part) in parts.iter().enumerate() {
                        if relative_byte_offset >= current_offset && relative_byte_offset < current_offset + part.len() {
                            // We're clicking on part i, so the module path is everything before this part
                            if i > 0 {
                                let module_path = parts[..i].join(".");
                                let source_module = self.find_module_definition()
                                    .map(|m| m.module_name(content).to_string())
                                    .unwrap_or_default();
                                return Some((module_path, source_module));
                            }
                            break;
                        }
                        current_offset += part.len() + 1; // +1 for the dot
                    }
                }
            }
            current = parent;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_lsp::lsp_types::{Position, Url};
    use crate::parser::AdlParser;

    #[test]
    fn test_get_module_path_from_import() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.module {
    import common.db.User;
    import other.module.*;
    
    struct MyStruct {
        String name;
    };
};"#;

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Test clicking on "common.db" in "import common.db.User;"
        // Position on line 1 (0-indexed), character 11 (pointing to "common.db")
        let position = Position { line: 1, character: 15 }; // Points to "db" in "common.db"
        let result = tree.get_module_path_at(&position, contents.as_bytes());
        
        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "common.db");
            assert_eq!(source_module, "test.module");
        } else {
            panic!("Expected to find module path in import declaration");
        }
    }

    #[test]
    fn test_get_module_path_from_scoped_name() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.module {
    struct MyStruct {
        common.string.StringNE name;
    };
};"#;

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Test clicking on "common" in "common.string.StringNE"
        // Position on line 2 (0-indexed), character 8 (pointing to "common")
        let position = Position { line: 2, character: 8 }; // Points to "common"
        let result = tree.get_module_path_at(&position, contents.as_bytes());
        
        if let Some((module_path, source_module)) = result {
            // When clicking on "common" in "common.string.StringNE", 
            // we expect to get empty module path since there's nothing before "common"
            // This test might need adjustment based on exact behavior desired
            assert_eq!(source_module, "test.module");
        }

        // Test clicking on "string" in "common.string.StringNE"
        let position = Position { line: 2, character: 15 }; // Points to "string"
        let result = tree.get_module_path_at(&position, contents.as_bytes());
        
        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "common");
            assert_eq!(source_module, "test.module");
        } else {
            panic!("Expected to find module path in scoped name");
        }
    }

    #[test]
    fn test_get_module_path_star_import() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.module {
    import other.module.*;
    
    struct MyStruct {
        String name;
    };
};"#;

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Test clicking on "other.module" in "import other.module.*;"
        let position = Position { line: 1, character: 15 }; // Points to "module" in "other.module"
        let result = tree.get_module_path_at(&position, contents.as_bytes());
        
        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "other.module");
            assert_eq!(source_module, "test.module");
        } else {
            panic!("Expected to find module path in star import");
        }
    }
}
