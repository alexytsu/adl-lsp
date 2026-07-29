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
    /// True when `node` is (or is part of) a `scoped_name` sitting in a `type_expression`'s
    /// `name:` slot whose text is an ADL primitive (`String`, `Vector`, ...).
    ///
    /// Grammar v0.7 has no dedicated `primitive_type` node — primitives parse as ordinary
    /// scoped names — so this is the replacement check: primitives have no user definition and
    /// goto-definition/hover lookups should skip them. A *field* named `String` is legal ADL
    /// and is not a type reference, so it is not matched here.
    pub fn is_primitive_type_reference(node: &Node<'_>, content: &[u8]) -> bool {
        let scoped_name = if NodeKind::is_scoped_name(node) {
            *node
        } else {
            match node.parent() {
                Some(parent) if NodeKind::is_scoped_name(&parent) => parent,
                _ => return false,
            }
        };

        let Some(type_expression) = scoped_name.parent() else {
            return false;
        };
        if !NodeKind::is_type_expression(&type_expression) {
            return false;
        }
        if type_expression.child_by_field_name("name").map(|n| n.id()) != Some(scoped_name.id()) {
            return false;
        }

        scoped_name
            .utf8_text(content)
            .ok()
            .is_some_and(crate::parser::primitives::is_primitive)
    }

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
                        if let Some(import_path) = parent.child_by_field_name("path") {
                            let mut path_cursor = import_path.walk();
                            if let Some(scoped_name) = import_path
                                .children(&mut path_cursor)
                                .find(NodeKind::is_scoped_name)
                            {
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

    /// If the cursor is inside the `field:` name of an `annotation_declaration`
    /// (e.g. the `title` in `annotation Message::title Doc "...";`), return the annotation's
    /// target type (possibly qualified, e.g. `common.db.User`) and the referenced field name.
    ///
    /// The caller decides how to resolve the target: locally via
    /// [`Self::find_field_definition_in_type`], or through the workspace import table for
    /// qualified/imported targets.
    pub fn get_annotation_field_reference_at<'a>(
        &'a self,
        pos: &Position,
        content: &'a [u8],
    ) -> Option<(&'a str, &'a str)> {
        let node = self.get_node_at_position(pos)?;
        // The cursor may be on the `field_name` node itself or its inner `identifier`.
        let field_name_node = if NodeKind::is_field_name(&node) {
            node
        } else {
            node.parent().filter(NodeKind::is_field_name)?
        };
        let annotation = field_name_node
            .parent()
            .filter(NodeKind::is_annotation_declaration)?;

        // Only the annotation's `field:` slot references a field; ignore other field_names.
        if annotation.child_by_field_name("field").map(|n| n.id()) != Some(field_name_node.id()) {
            return None;
        }

        let target_type = annotation
            .child_by_field_name("target")?
            .utf8_text(content)
            .ok()?;
        let field_name = field_name_node.utf8_text(content).ok()?;

        Some((target_type, field_name))
    }

    /// Resolve an annotation field reference at `pos` against types defined in this file only.
    /// Qualified targets (`a.b.Type::field`) return `None`; the server layer resolves those
    /// through the import table (see `Server::resolve_annotation_field_definition`, which
    /// composes `get_annotation_field_reference_at` + `find_field_definition_in_type`).
    #[cfg(test)]
    pub fn get_annotation_field_definition_at<'a>(
        &'a self,
        pos: &Position,
        content: &'a [u8],
    ) -> Option<Location> {
        let (target_type, field_name) = self.get_annotation_field_reference_at(pos, content)?;
        if target_type.contains('.') {
            return None;
        }
        self.find_field_definition_in_type(target_type, field_name, content)
    }

    /// Find the definition location of a field named `field_name` within a locally-defined
    /// struct/union named `type_name`.
    pub fn find_field_definition_in_type<'a>(
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
                let Some(name_node) = field.child_by_field_name("name") else {
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
        let contents = r#"module test.sample {
    import common.db.User;
    import other.sample.*;

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
            assert_eq!(source_module, "test.sample");
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
        let contents = r#"module test.sample {
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
            assert_eq!(source_module, "test.sample");
        }

        // Test clicking on "string" in "common.string.StringNE"
        let position = Position {
            line: 2,
            character: 15,
        }; // Points to "string"
        let result = tree.get_module_path_at(&position, contents.as_bytes());

        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "common");
            assert_eq!(source_module, "test.sample");
        } else {
            panic!("Expected to find module path in scoped name");
        }
    }

    #[test]
    fn test_get_module_path_star_import() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.sample {
    import other.sample.*;

    struct MyStruct {
        String name;
    };
};"#;

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // Test clicking on "other.sample" in "import other.sample.*;"
        let position = Position {
            line: 1,
            character: 15,
        }; // Points to "sample" in "other.sample"
        let result = tree.get_module_path_at(&position, contents.as_bytes());

        if let Some((module_path, source_module)) = result {
            assert_eq!(module_path, "other.sample");
            assert_eq!(source_module, "test.sample");
        } else {
            panic!("Expected to find module path in star import");
        }
    }

    #[test]
    fn test_annotation_field_reference_extraction() {
        let uri: Url = "file://annotations.adl".parse().unwrap();
        let contents = include_str!("input/annotations.adl");

        let mut parser = AdlParser::new();
        let tree = parser.parse(uri, contents.as_bytes()).unwrap();

        // `lastName` in `annotation Person::lastName SerializedName "ln";` (line 9).
        let reference = tree.get_annotation_field_reference_at(
            &Position {
                line: 9,
                character: 25,
            },
            contents.as_bytes(),
        );
        assert_eq!(reference, Some(("Person", "lastName")));

        // Local target resolves to the `lastName` field definition (line 6, char 15).
        let location = tree
            .get_annotation_field_definition_at(
                &Position {
                    line: 9,
                    character: 25,
                },
                contents.as_bytes(),
            )
            .expect("local annotation target should resolve");
        assert_eq!(location.range.start.line, 6);
        assert_eq!(location.range.start.character, 15);

        // Qualified target: `id` in `annotation common.db.User::id Doc ...` (line 11).
        let reference = tree.get_annotation_field_reference_at(
            &Position {
                line: 11,
                character: 31,
            },
            contents.as_bytes(),
        );
        assert_eq!(reference, Some(("common.db.User", "id")));
        // ... which is not resolvable locally (server resolves it via the import table).
        assert!(
            tree.get_annotation_field_definition_at(
                &Position {
                    line: 11,
                    character: 31,
                },
                contents.as_bytes(),
            )
            .is_none()
        );

        // A module-target annotation without a `::field` is not a field reference (line 12).
        assert!(
            tree.get_annotation_field_reference_at(
                &Position {
                    line: 12,
                    character: 20,
                },
                contents.as_bytes(),
            )
            .is_none()
        );
    }

    #[test]
    fn test_annotation_field_goto() {
        let uri: Url = "file://test.adl".parse().unwrap();
        let contents = r#"module test.sample {
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
