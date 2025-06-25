mod kind;
pub use kind::NodeKind;
use tracing::warn;
use tree_sitter::Node;

/// Type-safe helpers to perform operations on known-node types
#[allow(dead_code)]
pub enum AdlNode<'a> {
    Import(AdlImportDeclaration<'a>),
    ScopedName(AdlScopedName<'a>),
    ModuleDefinition(AdlModuleDefinition<'a>),
}

/// A module definition provides a namespace for types and functions that are defined within it
///
/// ### Grammar definition
/// ```javascript
/// module_definition: ($) => seq(
///   optional($.definition_preamble),
///   "module",
///   $.scoped_name,
///   $.module_body
/// ),
/// ```
#[derive(Debug)]
pub struct AdlModuleDefinition<'a> {
    node: Node<'a>,
}

impl<'a> AdlModuleDefinition<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_module_definition(&node) {
            Some(AdlModuleDefinition { node })
        } else {
            warn!("expected a module definition node but got {}", node.kind());
            None
        }
    }

    pub fn module_name<'b>(&self, content: &'b [u8]) -> &'b str {
        let module_or_preamble = self
            .node
            .child(0)
            .expect("module_definition should have children");
        let scoped_name = if NodeKind::is_definition_preamble(&module_or_preamble) {
            self.node.child(2).expect("expected scoped_name")
        } else {
            self.node.child(1).expect("expected scoped_name")
        };
        scoped_name.utf8_text(content).expect("utf-8 parse error")
    }
}

/// An import declaration appears at the top of a module and can either be a fully qualified name or a star-import.
///
/// ### Grammar definition
/// ```javascript
/// import_declaration: ($) => seq("import", $.import_path, ";"),
/// import_path: ($) => seq($.scoped_name, optional(".*")),
/// ```
#[derive(Debug, Clone)]
pub enum AdlImportDeclaration<'a> {
    FullyQualified(AdlScopedName<'a>),
    StarImport(AdlScopedName<'a>),
}

impl<'a> AdlImportDeclaration<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_import_declaration(&node) {
            if Self::is_star_import(&node) {
                Some(AdlImportDeclaration::StarImport(AdlScopedName { node }))
            } else {
                Some(AdlImportDeclaration::FullyQualified(AdlScopedName { node }))
            }
        } else {
            None
        }
    }

    pub fn is_star_import(node: &Node<'a>) -> bool {
        node.child(1)
            .expect("expected import_path")
            .child(1)
            .is_some() // either scoped name or scoped_name.*
    }

    pub fn module_name<'b>(&self, content: &'b [u8]) -> &'b str {
        match self {
            AdlImportDeclaration::FullyQualified(scoped_name) => {
                let full_name = scoped_name
                    .node
                    .child(1)
                    .expect("expected import_path")
                    .utf8_text(content)
                    .expect("utf-8 parse error");
                // Remove the last part (type name) from fully qualified names
                // e.g., "mod.foo.Type" -> "mod.foo"
                if let Some(last_dot_pos) = full_name.rfind('.') {
                    &full_name[..last_dot_pos]
                } else {
                    full_name
                }
            }
            AdlImportDeclaration::StarImport(scoped_name) => scoped_name
                .node
                .child(1)
                .expect("expected import_path")
                .child(0) // drop the .*
                .expect("expected scoped_name")
                .utf8_text(content)
                .expect("utf-8 parse error"),
        }
    }

    pub fn imported_type_name<'b>(&self, content: &'b [u8]) -> Option<&'b str> {
        match self {
            AdlImportDeclaration::FullyQualified(scoped_name) => scoped_name
                .node
                .child(1)
                .expect("expected import_path")
                .utf8_text(content)
                .expect("utf-8 parse error")
                .split('.')
                .last(),
            AdlImportDeclaration::StarImport(_) => None,
        }
    }
}

/// A scoped name is a sequence of identifiers separated by dots
/// It is used to define module paths and fully-qualified type names in ADL
///
/// ### Grammar definition
/// ```javascript
/// scoped_name: ($) => seq($.identifier, repeat(seq(".", $.identifier))),
/// ```
#[derive(Debug, Clone)]
pub struct AdlScopedName<'a> {
    node: Node<'a>,
}

#[cfg(test)]
mod test {
    use lsp_types::Url;

    use super::AdlImportDeclaration;
    use crate::node::NodeKind;
    use crate::parser::{AdlParser, tree::Tree};

    const SAMPLE_ADL: &str = r#"
    module foo {
        import foo.bar.Baz;
        import foo.char.*;
    };
    "#;

    #[test]
    pub fn test_module_definition_basic() {
        let mut parser = AdlParser::new();
        let uri: Url = "file://input/message.adl".parse().unwrap();
        let tree = parser.parse(uri, SAMPLE_ADL).unwrap();
        let module_definition = tree
            .find_first_node(NodeKind::is_module_definition)
            .expect("expected module definition");
        let module_name = super::AdlModuleDefinition::try_new(module_definition)
            .expect("expected module definition")
            .module_name(SAMPLE_ADL.as_bytes());
        assert_eq!(module_name, "foo");
    }

    #[test]
    pub fn test_import_declaration_basic() {
        let mut parser = AdlParser::new();
        let uri: Url = "file://input/message.adl".parse().unwrap();
        let tree = parser.parse(uri, SAMPLE_ADL).unwrap();
        let import_declarations = tree
            .find_all_nodes(NodeKind::is_import_declaration)
            .iter()
            .filter_map(|n| AdlImportDeclaration::try_new(*n))
            .collect::<Vec<_>>();

        assert_eq!(import_declarations.len(), 2);
        assert!(matches!(
            import_declarations[0],
            AdlImportDeclaration::FullyQualified(_)
        ));
        assert!(matches!(
            import_declarations[1],
            AdlImportDeclaration::StarImport(_)
        ));

        assert_eq!(
            import_declarations[0].module_name(SAMPLE_ADL.as_bytes()),
            "foo.bar"
        );
        assert_eq!(
            import_declarations[1].module_name(SAMPLE_ADL.as_bytes()),
            "foo.char"
        );
        assert_eq!(
            import_declarations[0].imported_type_name(SAMPLE_ADL.as_bytes()),
            Some("Baz")
        );
        assert_eq!(
            import_declarations[1].imported_type_name(SAMPLE_ADL.as_bytes()),
            None
        );
    }
}
