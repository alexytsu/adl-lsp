use async_lsp::lsp_types::{Location, Position, Range};
use tree_sitter::{Node, TreeCursor};

use crate::parser::ts_lsp_interop as ts_lsp;
use crate::{node::NodeKind, parser::ParsedTree};

enum ImportNavigation {
    Module((String, String)),
    Type,
}

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
        if let Some(import_info) = self.get_module_from_import_at_position(&node, content, pos) {
            match import_info {
                ImportNavigation::Module(info) => return Some(info),
                ImportNavigation::Type => return None,
            }
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
        pos: &Position,
    ) -> Option<ImportNavigation> {
        // Walk up the tree to find if we're in an import declaration
        let mut current = *node;
        while let Some(parent) = current.parent() {
            if NodeKind::is_import_declaration(&parent) {
                if let Some(import_decl) = crate::node::AdlImportDeclaration::try_new(parent) {
                    if matches!(
                        import_decl,
                        crate::node::AdlImportDeclaration::FullyQualified(_)
                    ) {
                        if let Some(import_path) = parent.child(1) {
                            if let Some(scoped_name) = import_path.child(0) {
                                if let Some((segment_index, parts)) =
                                    Self::scoped_name_segment_index_at_position(
                                        &scoped_name,
                                        &current,
                                        pos,
                                        content,
                                    )
                                {
                                    if segment_index + 1 == parts.len() {
                                        // Cursor is on the imported type name; let identifier-based
                                        // navigation handle goto definition instead of module navigation.
                                        return Some(ImportNavigation::Type);
                                    }
                                }
                            }
                        }
                    }

                    let source_module = self
                        .find_module_definition()
                        .map(|m| m.module_name(content).to_string())
                        .unwrap_or_default();
                    let module_path = import_decl.module_name(content).to_string();
                    return Some(ImportNavigation::Module((module_path, source_module)));
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
                if let Some((segment_index, parts)) =
                    Self::scoped_name_segment_index_at_position(&parent, &current, pos, content)
                {
                    if segment_index > 0 {
                        let module_path = parts[..segment_index].join(".");
                        let source_module = self
                            .find_module_definition()
                            .map(|m| m.module_name(content).to_string())
                            .unwrap_or_default();
                        return Some((module_path, source_module));
                    }
                }
            }
            current = parent;
        }
        None
    }

    fn scoped_name_segment_index_at_position(
        scoped_node: &Node<'_>,
        current: &Node<'_>,
        pos: &Position,
        content: &[u8],
    ) -> Option<(usize, Vec<String>)> {
        let scoped_text = scoped_node.utf8_text(content).ok()?;

        let parts: Vec<String> = scoped_text.split('.').map(str::to_string).collect();
        if parts.len() <= 1 {
            return None;
        }

        // Convert cursor position to byte offset within the scoped name
        let cursor_point = ts_lsp::lsp_to_ts_point(pos);
        let scoped_start_point = scoped_node.start_position();

        // Calculate relative byte offset within the scoped name text
        let relative_byte_offset = if cursor_point.row == scoped_start_point.row {
            cursor_point
                .column
                .saturating_sub(scoped_start_point.column)
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
            if relative_byte_offset >= current_offset
                && relative_byte_offset < current_offset + part.len()
            {
                return Some((i, parts));
            }
            current_offset += part.len() + 1; // +1 for the dot
        }

        None
    }

    /// If the cursor is on the identifier of a `field_reference` inside an `annotation_declaration`
    /// (e.g. the `title` in `annotation Message::title Doc "...";`), resolve it to the referenced
    /// field's definition.
    ///
    /// Only locally-defined, unqualified target types are resolved for now; qualified/imported
    /// target types (e.g. `common.db.User::field`) return `None` and fall through to the caller.
    pub fn get_annotation_field_definition_at<'a>(
        &'a self,
        pos: &Position,
        content: &'a [u8],
    ) -> Option<Location> {
        let node = self.get_node_at_position(pos)?;
        if !NodeKind::is_identifier(&node) {
            return None;
        }

        let field_reference = node.parent().filter(NodeKind::is_field_reference)?;
        let annotation = field_reference
            .parent()
            .filter(NodeKind::is_annotation_declaration)?;
        let field_name = node.utf8_text(content).ok()?;

        // The first scoped_name child of the annotation is its target type.
        let mut cursor = annotation.walk();
        let target_type = annotation
            .children(&mut cursor)
            .find(|child| NodeKind::is_scoped_name(child))?
            .utf8_text(content)
            .ok()?;

        // TODO(low): resolve qualified/imported annotation targets via the import table (would
        // require threading the server's import resolution into this path).
        if target_type.contains('.') {
            return None;
        }

        self.find_field_definition_in_type(target_type, field_name, content)
    }

    /// Find the definition location of a field named `field_name` within a locally-defined
    /// struct/union named `type_name`.
    fn find_field_definition_in_type<'a>(
        &'a self,
        type_name: &str,
        field_name: &str,
        content: &'a [u8],
    ) -> Option<Location> {
        for definition in self.find_all_nodes(NodeKind::is_local_definition) {
            if crate::node::definition_type_name(&definition, content) != Some(type_name) {
                continue;
            }

            for field in self.find_all_nodes_from(definition, NodeKind::is_field) {
                // The field's name is its direct `identifier` child (the type_expression comes
                // before it and is a distinct node kind).
                let mut field_cursor = field.walk();
                let Some(name_node) = field
                    .children(&mut field_cursor)
                    .find(NodeKind::is_identifier)
                else {
                    continue;
                };
                if name_node.utf8_text(content).ok() == Some(field_name) {
                    return Some(Location {
                        uri: self.uri.clone(),
                        range: Range {
                            start: ts_lsp::ts_to_lsp_position(&name_node.start_position()),
                            end: ts_lsp::ts_to_lsp_position(&name_node.end_position()),
                        },
                    });
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::AdlParser;
    use async_lsp::lsp_types::{Position, Url};

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
        let position = Position {
            line: 1,
            character: 15,
        }; // Points to "db" in "common.db"
        let result = tree.get_module_path_at(&position, contents.as_bytes());

        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "common.db");
            assert_eq!(source_module, "test.module");
        } else {
            panic!("Expected to find module path in import declaration");
        }

        // Test clicking on "User" in "import common.db.User;" should not navigate to module
        let position = Position {
            line: 1,
            character: 22,
        }; // Points to "User"
        let result = tree.get_module_path_at(&position, contents.as_bytes());
        assert!(result.is_none());
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
        let position = Position {
            line: 2,
            character: 8,
        }; // Points to "common"
        let result = tree.get_module_path_at(&position, contents.as_bytes());

        if let Some((_module_path, source_module)) = result {
            // When clicking on "common" in "common.string.StringNE",
            // we expect to get empty module path since there's nothing before "common"
            // This test might need adjustment based on exact behavior desired
            assert_eq!(source_module, "test.module");
        }

        // Test clicking on "string" in "common.string.StringNE"
        let position = Position {
            line: 2,
            character: 15,
        }; // Points to "string"
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
        let position = Position {
            line: 1,
            character: 15,
        }; // Points to "module" in "other.module"
        let result = tree.get_module_path_at(&position, contents.as_bytes());

        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "other.module");
            assert_eq!(source_module, "test.module");
        } else {
            panic!("Expected to find module path in star import");
        }
    }

    #[test]
    fn test_annotation_field_goto() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.module {
    struct Message {
        String title;
        String body;
    };

    annotation Message::title Doc "documentation";
};"#;

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Cursor on `title` in `annotation Message::title ...` (line 6) resolves to the `title`
        // field definition on line 2.
        let position = Position {
            line: 6,
            character: 26,
        };
        let location = tree
            .get_annotation_field_definition_at(&position, contents.as_bytes())
            .expect("expected annotation field reference to resolve to the field definition");
        assert_eq!(location.range.start.line, 2);
        assert_eq!(location.range.start.character, 15);

        // Cursor on the annotation type `Doc` is not a field reference.
        let position = Position {
            line: 6,
            character: 31,
        };
        assert!(
            tree.get_annotation_field_definition_at(&position, contents.as_bytes())
                .is_none()
        );
    }
}
