mod kind;

pub use kind::NodeKind;
use tracing::warn;
use tree_sitter::{Node, TreeCursor};

/// Shared heuristic for declarations that should be terminated by `;`: the node is considered
/// to be missing its semicolon when its last child is not a `;` token.
///
/// Centralised here so every typed wrapper and the diagnostics pass share one definition.
pub fn is_missing_semicolon(node: &Node<'_>) -> bool {
    let mut cursor = node.walk();
    let last_child = cursor.goto_last_child();
    last_child && cursor.node().kind() != ";"
}

/// Extract the declared type name from a definition node (`type`/`newtype`/`struct`/`union`).
///
/// Every ADL definition names its type via a single `type_name` child; this returns its text.
/// Centralises the child-walk that would otherwise be duplicated at each call site.
pub fn definition_type_name<'b>(node: &Node<'_>, content: &'b [u8]) -> Option<&'b str> {
    node.child_by_field_name("name")
        .filter(NodeKind::is_type_name)
        .and_then(|type_name_node| type_name_node.utf8_text(content).ok())
}

/// Extract a definition's version tag (the `#2` in `struct X#2 { ... }`), if present.
///
/// Grammar v0.7 exposes this as the optional `version:` field on
/// `struct`/`union`/`type`/`newtype` definitions; its text includes the leading `#`.
pub fn definition_version<'b>(node: &Node<'_>, content: &'b [u8]) -> Option<&'b str> {
    node.child_by_field_name("version")
        .and_then(|version_node| version_node.utf8_text(content).ok())
}

/// Type-safe helpers to perform operations on known-node types
#[allow(dead_code)]
pub enum AdlNode<'a> {
    Import(AdlImportDeclaration<'a>),
    ScopedName(AdlScopedName<'a>),
    ModuleDefinition(AdlModuleDefinition<'a>),
    ModuleBody(AdlModuleBody<'a>),
    TypeDefinition(AdlTypeDefinition<'a>),
    NewtypeDefinition(AdlNewtypeDefinition<'a>),
    StructDefinition(AdlStructDefinition<'a>),
    UnionDefinition(AdlUnionDefinition<'a>),
    Field(AdlField<'a>),
    AnnotationDeclaration(AdlAnnotationDeclaration<'a>),
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
        self.node
            .child_by_field_name("name")
            .expect("module_definition should have a name: field")
            .utf8_text(content)
            .expect("utf-8 parse error")
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

    /// The `path:` field of the import declaration (an `import_path` node).
    fn import_path(node: &Node<'a>) -> Node<'a> {
        node.child_by_field_name("path")
            .expect("import_declaration should have a path: field")
    }

    /// A star import's `import_path` ends with a `.*` wildcard token after the scoped name.
    pub fn is_star_import(node: &Node<'a>) -> bool {
        let path = Self::import_path(node);
        let mut cursor = path.walk();
        path.children(&mut cursor)
            .any(|child| child.kind().contains('*'))
    }

    /// Text of the scoped name inside the import path (the wildcard token excluded).
    fn scoped_name_text<'b>(&self, content: &'b [u8]) -> &'b str {
        let node = match self {
            AdlImportDeclaration::FullyQualified(inner)
            | AdlImportDeclaration::StarImport(inner) => &inner.node,
        };
        let path = Self::import_path(node);
        let mut cursor = path.walk();
        path.children(&mut cursor)
            .find(NodeKind::is_scoped_name)
            .expect("import_path should contain a scoped_name")
            .utf8_text(content)
            .expect("utf-8 parse error")
    }

    /// The module part of the import.
    ///
    /// A `scoped_name` does not distinguish module segments from the trailing type name, so the
    /// split is inherently textual: for fully-qualified imports the final segment is the type
    /// name (e.g. `mod.foo.Type` -> `mod.foo`); star imports name the module directly.
    pub fn module_name<'b>(&self, content: &'b [u8]) -> &'b str {
        let full_name = self.scoped_name_text(content);
        match self {
            AdlImportDeclaration::FullyQualified(_) => match full_name.rfind('.') {
                Some(last_dot_pos) => &full_name[..last_dot_pos],
                None => full_name,
            },
            AdlImportDeclaration::StarImport(_) => full_name,
        }
    }

    /// The imported type name (final segment) for fully-qualified imports; `None` for `.*`.
    pub fn imported_type_name<'b>(&self, content: &'b [u8]) -> Option<&'b str> {
        match self {
            AdlImportDeclaration::FullyQualified(_) => {
                self.scoped_name_text(content).split('.').next_back()
            }
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

/// A module body contains imports, type definitions and annotation declarations
///
/// ## Grammar Definition
/// ```javascript
/// module_body: ($) =>
///   seq(
///     "{",
///     repeat(
///       choice(
///         $.import_declaration,
///         $.annotation_declaration,
///         $.type_definition,
///         $.newtype_definition,
///         $.struct_definition,
///         $.union_definition
///       )
///     ),
///     "}"
/// )
/// ```
#[derive(Debug)]
pub struct AdlModuleBody<'a> {
    node: Node<'a>,
}

impl<'a> AdlModuleBody<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_module_body(&node) {
            Some(AdlModuleBody { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }
}

/// A type definition defines a new type
///
/// ### Grammar definition
/// ```javascript
/// type_definition: ($) =>
///   seq(
///     optional($.definition_preamble),
///     "type",
///     $.type_name,
///     optional($.type_parameters),
///     "=",
///     $.type_expression,
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlTypeDefinition<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlTypeDefinition<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_type_definition(&node) {
            Some(AdlTypeDefinition { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}

/// A newtype definition defines a new type with a default value
///
/// ### Grammar definition
/// ```javascript
/// newtype_definition: ($) =>
///   seq(
///     optional($.definition_preamble),
///     "newtype",
///     $.type_name,
///     optional($.type_parameters),
///     "=",
///     $.type_expression,
///     optional(seq("=", $.json_value)),
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlNewtypeDefinition<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlNewtypeDefinition<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_newtype_definition(&node) {
            Some(AdlNewtypeDefinition { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}

/// A struct definition defines a new type with named fields
///
/// ### Grammar definition
/// ```javascript
/// struct_definition: ($) =>
///   seq(
///     optional($.definition_preamble),
///     "struct",
///     $.type_name,
///     optional($.type_parameters),
///     $.field_block,
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlStructDefinition<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlStructDefinition<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_struct_definition(&node) {
            Some(AdlStructDefinition { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}

/// A union definition defines a new type with named fields
///
/// ### Grammar definition
/// ```javascript
/// union_definition: ($) =>
///   seq(
///     optional($.definition_preamble),
///     "union",
///     $.type_name,
///     optional($.type_parameters),
///     $.field_block,
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlUnionDefinition<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlUnionDefinition<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_union_definition(&node) {
            Some(AdlUnionDefinition { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}

/// A field defines a named field in a struct or union
///
/// ### Grammar definition
/// ```javascript
/// field: ($) =>
///   seq(
///     optional($.definition_preamble),
///     $.type_expression,
///     $.identifier,
///     optional(seq("=", $.json_value)),
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlField<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlField<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_field(&node) {
            Some(AdlField { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}

/// An annotation declaration defines a new annotation
///
/// ### Grammar definition
/// ```javascript
/// annotation_declaration: ($) =>
///   seq(
///     "annotation",
///     seq($.scoped_name, repeat(seq("::", $.field_reference))),
///     $.scoped_name,
///     $.json_value,
///     optional(";")
///   )
/// ```
#[allow(dead_code)]
#[derive(Debug)]
pub struct AdlAnnotationDeclaration<'a> {
    node: Node<'a>,
}

#[allow(dead_code)]
impl<'a> AdlAnnotationDeclaration<'a> {
    pub fn try_new(node: Node<'a>) -> Option<Self> {
        if NodeKind::is_annotation_declaration(&node) {
            Some(AdlAnnotationDeclaration { node })
        } else {
            None
        }
    }

    pub fn cursor(&self) -> TreeCursor {
        self.node.walk()
    }

    pub fn is_missing_semicolon(&self) -> bool {
        is_missing_semicolon(&self.node)
    }
}
