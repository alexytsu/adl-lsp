use async_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};
use tree_sitter::Node;

use crate::node::NodeKind;
use crate::parser::ParsedTree;
use crate::parser::ts_lsp_interop;

pub trait DocumentSymbols {
    fn collect_document_symbols(&self, content: &[u8]) -> Vec<DocumentSymbol>;
}

impl DocumentSymbols for ParsedTree {
    fn collect_document_symbols(&self, content: &[u8]) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();
        self.collect_symbols_from_node(self.tree.root_node(), content, &mut symbols);
        symbols
    }
}

impl ParsedTree {
    fn collect_symbols_from_node(
        &self,
        node: Node<'_>,
        content: &[u8],
        symbols: &mut Vec<DocumentSymbol>,
    ) {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let symbol = self.node_to_symbol(&child, content);

            if let Some(mut symbol) = symbol {
                // Collect children recursively
                let mut children = Vec::new();
                self.collect_symbols_from_node(child, content, &mut children);

                if !children.is_empty() {
                    symbol.children = Some(children);
                }

                symbols.push(symbol);
            } else {
                // If this node isn't a symbol, check its children
                self.collect_symbols_from_node(child, content, symbols);
            }
        }
    }

    fn node_to_symbol(&self, node: &Node<'_>, content: &[u8]) -> Option<DocumentSymbol> {
        // Use the actual node kind string instead of mapping through NodeKind enum
        match NodeKind::from_kind(node.kind()) {
            NodeKind::ModuleDefinition => self.create_symbol(node, content, SymbolKind::MODULE),
            // NOTE: unsure if SymbolKind::CLASS is the best representation of type decls
            NodeKind::TypeDefinition => self.create_symbol(node, content, SymbolKind::CLASS),
            NodeKind::NewtypeDefinition => self.create_symbol(node, content, SymbolKind::CLASS),
            NodeKind::StructDefinition => self.create_symbol(node, content, SymbolKind::STRUCT),
            NodeKind::UnionDefinition => self.create_symbol(node, content, SymbolKind::ENUM),
            NodeKind::Field => match node.parent().and_then(|parent| parent.parent()) {
                Some(parent) => {
                    if NodeKind::is_struct_definition(&parent) {
                        self.create_symbol(node, content, SymbolKind::FIELD)
                    } else if NodeKind::is_union_definition(&parent) {
                        self.create_symbol(node, content, SymbolKind::ENUM_MEMBER)
                    } else {
                        None
                    }
                }
                None => self.create_symbol(node, content, SymbolKind::FIELD),
            },
            NodeKind::AnnotationDeclaration => {
                self.create_symbol(node, content, SymbolKind::STRUCT)
            }
            _ => None,
        }
    }

    fn create_symbol(
        &self,
        node: &Node<'_>,
        content: &[u8],
        symbol_kind: SymbolKind,
    ) -> Option<DocumentSymbol> {
        let range = ts_lsp_interop::ts_to_lsp_range(&node.range());
        let selection_range = Self::extract_selection_range(node).unwrap_or(range);

        // unknown would be a bug, but don't panic here
        let name = Self::extract_symbol_name(node, content).unwrap_or_else(|| "unknown".into());
        let detail = Self::extract_details(node, content);

        #[allow(deprecated)]
        Some(DocumentSymbol {
            name,
            detail,
            kind: symbol_kind,
            tags: None,
            range,
            selection_range,
            children: None,
            deprecated: None,
        })
    }

    /// Extract the symbol's display name.
    ///
    /// Named declarations expose their name via the grammar's `name:` field; versioned
    /// declarations (`struct X#2 { ... }`) render as `X#2`. Nodes without a `name:` field fall
    /// back to the first identifier/scoped_name found in a child walk.
    fn extract_symbol_name(node: &Node<'_>, content: &[u8]) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = name_node.utf8_text(content).ok()?;
            // Versioned decls render as `X#2` (the version text includes the leading `#`).
            return match crate::node::definition_version(node, content) {
                Some(version) => Some(format!("{name}{version}")),
                None => Some(name.to_string()),
            };
        }

        Self::find_first_name_text(node, content).map(str::to_string)
    }

    /// Fallback name extraction: recursively search for the first identifier or scoped_name.
    fn find_first_name_text<'a>(node: &Node<'a>, content: &'a [u8]) -> Option<&'a str> {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if NodeKind::is_identifier(&child) || NodeKind::is_scoped_name(&child) {
                return child.utf8_text(content).ok();
            }
            if let Some(name) = Self::find_first_name_text(&child, content) {
                return Some(name);
            }
        }

        None
    }

    fn extract_details(node: &Node<'_>, content: &[u8]) -> Option<String> {
        // `type X = <type>`, `newtype X = <type>` and `<type> fieldName` all expose the
        // type expression via the grammar's `type:` field.
        if NodeKind::is_type_definition(node)
            || NodeKind::is_newtype_definition(node)
            || NodeKind::is_field(node)
        {
            return node
                .child_by_field_name("type")
                .and_then(|type_node| type_node.utf8_text(content).ok())
                .map(str::to_string);
        }

        None
    }

    fn extract_selection_range(node: &Node<'_>) -> Option<Range> {
        // The declaration's own name (via the grammar's `name:` field) is the selection range.
        if let Some(name_node) = node.child_by_field_name("name") {
            return Some(ts_lsp_interop::ts_to_lsp_range(&name_node.range()));
        }

        // Fallback: descend first children looking for an identifier.
        let mut cursor = node.walk();
        if let Some(child) = node.children(&mut cursor).next() {
            if NodeKind::is_identifier(&child) {
                return Some(ts_lsp_interop::ts_to_lsp_range(&child.range()));
            }

            return Self::extract_selection_range(&child);
        }

        None
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::AdlParser;
    use async_lsp::lsp_types::Url;

    #[test]
    fn test_document_symbols() {
        let mut parser = AdlParser::new();
        let uri = Url::parse("file:///test.adl").unwrap();
        let content = r#"
            module sample.simple {
                struct Person {
                    String name;
                    Int age;
                };

                union Color {
                    Void red;
                    Void green;
                    Void blue;
                };

                type UserId = Int32;

                newtype Email = String;
            };"#;

        let tree = parser.parse(uri, content.as_bytes()).unwrap();
        let symbols = tree.collect_document_symbols(content.as_bytes());

        // Check for module symbol
        let module_symbol = symbols.iter().find(|s| s.kind == SymbolKind::MODULE);
        let module_symbol = module_symbol.unwrap();
        assert_eq!(module_symbol.name, "sample.simple");

        module_symbol.children.iter().for_each(|module_children| {
            let struct_symbol = module_children
                .iter()
                .find(|s| s.kind == SymbolKind::STRUCT);
            let struct_symbol = struct_symbol.unwrap();
            assert_eq!(struct_symbol.name, "Person");
            assert_eq!(struct_symbol.detail, None);

            let union_symbol = module_children
                .iter()
                .find(|s| s.kind == SymbolKind::ENUM)
                .unwrap();
            assert_eq!(union_symbol.name, "Color");
            assert_eq!(union_symbol.detail, None);

            let type_symbol = module_children.iter().find(|s| s.name == "UserId").unwrap();
            assert_eq!(type_symbol.kind, SymbolKind::CLASS);
            assert_eq!(type_symbol.detail, Some("Int32".to_string()));

            let newtype_symbol = module_children.iter().find(|s| s.name == "Email").unwrap();
            assert_eq!(newtype_symbol.kind, SymbolKind::CLASS);
            assert_eq!(newtype_symbol.detail, Some("String".to_string()));
        });
    }

    #[test]
    fn test_document_symbols_snapshot() {
        let mut parser = AdlParser::new();
        let uri = Url::parse("file:///test.adl").unwrap();
        let content = r#"
            module sample.simple {
                struct Person {
                    String name;
                    Int age;
                };

                union Color {
                    Void red;
                    Void green;
                    Void blue;
                };

                type UserId = Int32;

                newtype Email = String;
            };"#;

        let tree = parser.parse(uri, content.as_bytes()).unwrap();
        let symbols = tree.collect_document_symbols(content.as_bytes());
        insta::assert_yaml_snapshot!(symbols);
    }

    /// Versioned declarations render as `Name#version` in the symbol tree.
    #[test]
    fn test_versioned_symbols_snapshot() {
        let mut parser = AdlParser::new();
        let uri = Url::parse("file:///versioned.adl").unwrap();
        let content = include_str!("input/versioned.adl");

        let tree = parser.parse(uri, content.as_bytes()).unwrap();
        let symbols = tree.collect_document_symbols(content.as_bytes());

        let module = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::MODULE)
            .expect("module symbol");
        let names: Vec<&str> = module
            .children
            .iter()
            .flatten()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"Settings#1"), "names: {names:?}");
        assert!(names.contains(&"Settings#2"), "names: {names:?}");
        assert!(names.contains(&"Shape#1"), "names: {names:?}");
        assert!(names.contains(&"Alias#2"), "names: {names:?}");
        assert!(names.contains(&"Consumer"), "names: {names:?}");

        insta::assert_yaml_snapshot!(symbols);
    }

    /// A field named `String` (legal ADL) and newtype-with-default keep clean symbols.
    #[test]
    fn test_misc_v07_symbols_snapshot() {
        let mut parser = AdlParser::new();
        let uri = Url::parse("file:///misc_v07.adl").unwrap();
        let content = include_str!("input/misc_v07.adl");

        let tree = parser.parse(uri, content.as_bytes()).unwrap();
        let symbols = tree.collect_document_symbols(content.as_bytes());
        insta::assert_yaml_snapshot!(symbols);
    }
}
