#![allow(dead_code)]
//! The full grammar sepcification the of ADL's AST from tree-sitter-adl
use tree_sitter::Node;

/// Typed parsing of node.kind() strings from a tree-sitter parse tree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    // Comments and Documentation
    Comment,
    Docstring,
    DefinitionPreamble,

    // Identifiers and Names
    Identifier,
    ScopedName,
    TypeName,

    // Type System
    TypeParameters,
    TypeArguments,
    TypeExpression,
    PrimitiveType,

    // Type Definitions
    TypeDefinition,
    NewtypeDefinition,
    StructDefinition,
    UnionDefinition,
    Field,
    FieldBlock,

    // Module System
    ModuleDefinition,
    ModuleBody,
    ImportDeclaration,
    ImportPath,

    // Annotations
    Annotation,
    Annotations,
    AnnotationDeclaration,
    FieldReference,

    // JSON
    JsonValue,
    JsonNumber,
    JsonString,
    JsonArray,
    JsonObject,
    JsonObjectPair,

    // Error
    Error,
}

impl NodeKind {
    pub fn is_user_defined_name(n: &Node) -> bool {
        Self::is_identifier(n) || Self::is_type_name(n) || Self::is_scoped_name(n)
    }

    pub fn is_local_definition(n: &Node) -> bool {
        Self::is_type_definition(n)
            || Self::is_newtype_definition(n)
            || Self::is_struct_definition(n)
            || Self::is_union_definition(n)
    }

    pub fn is_definition(n: &Node) -> bool {
        Self::is_import_declaration(n) || Self::is_type_name(n) || Self::is_import_declaration(n)
    }

    /// Check if an identifier is part of a scoped name
    pub fn has_scoped_name_parent(n: &Node) -> bool {
        Self::is_identifier(n) && n.parent().is_some_and(|p| Self::is_scoped_name(&p))
    }

    /// Check if a node represents a valid identifier definition that can be referenced
    /// (i.e., an identifier that is part of a type_name)
    pub fn can_be_referenced(n: &Node) -> bool {
        Self::is_identifier(n) && n.parent().is_some_and(|p| Self::is_type_name(&p))
    }
}

impl NodeKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            // Comments and Documentation
            "comment" => Self::Comment,
            "docstring" => Self::Docstring,
            "definition_preamble" => Self::DefinitionPreamble,

            // Identifiers and Names
            "identifier" => Self::Identifier,
            "scoped_name" => Self::ScopedName,
            "type_name" => Self::TypeName,

            // Type System
            "type_parameters" => Self::TypeParameters,
            "type_arguments" => Self::TypeArguments,
            "type_expression" => Self::TypeExpression,
            "primitive_type" => Self::PrimitiveType,

            // Type Definitions
            "type_definition" => Self::TypeDefinition,
            "newtype_definition" => Self::NewtypeDefinition,
            "struct_definition" => Self::StructDefinition,
            "union_definition" => Self::UnionDefinition,
            "field" => Self::Field,
            "field_block" => Self::FieldBlock,

            // Module System
            "module_definition" => Self::ModuleDefinition,
            "module_body" => Self::ModuleBody,
            "import_declaration" => Self::ImportDeclaration,
            "import_path" => Self::ImportPath,

            // Annotations
            "annotation" => Self::Annotation,
            "annotations" => Self::Annotations,
            "annotation_declaration" => Self::AnnotationDeclaration,
            "field_reference" => Self::FieldReference,

            // JSON
            "json_value" => Self::JsonValue,
            "json_number" => Self::JsonNumber,
            "json_string" => Self::JsonString,
            "json_array" => Self::JsonArray,
            "json_object" => Self::JsonObject,
            "json_object_pair" => Self::JsonObjectPair,

            // Default to Error for unknown kinds
            _ => Self::Error,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            // Comments and Documentation
            Self::Comment => "comment",
            Self::Docstring => "docstring",
            Self::DefinitionPreamble => "definition_preamble",

            // Identifiers and Names
            Self::Identifier => "identifier",
            Self::ScopedName => "scoped_name",
            Self::TypeName => "type_name",

            // Type System
            Self::TypeParameters => "type_parameters",
            Self::TypeArguments => "type_arguments",
            Self::TypeExpression => "type_expression",
            Self::PrimitiveType => "primitive_type",

            // Type Definitions
            Self::TypeDefinition => "type_definition",
            Self::NewtypeDefinition => "newtype_definition",
            Self::StructDefinition => "struct_definition",
            Self::UnionDefinition => "union_definition",
            Self::Field => "field",
            Self::FieldBlock => "field_block",

            // Module System
            Self::ModuleDefinition => "module_definition",
            Self::ModuleBody => "module_body",
            Self::ImportDeclaration => "import_declaration",
            Self::ImportPath => "import_path",

            // Annotations
            Self::Annotation => "annotation",
            Self::Annotations => "annotations",
            Self::AnnotationDeclaration => "annotation_declaration",
            Self::FieldReference => "field_reference",

            // JSON
            Self::JsonValue => "json_value",
            Self::JsonNumber => "json_number",
            Self::JsonString => "json_string",
            Self::JsonArray => "json_array",
            Self::JsonObject => "json_object",
            Self::JsonObjectPair => "json_object_pair",

            // Error
            Self::Error => "ERROR",
        }
    }

    // Comments and Documentation
    pub fn is_comment(n: &Node) -> bool {
        n.kind() == Self::Comment.as_str()
    }

    pub fn is_docstring(n: &Node) -> bool {
        n.kind() == Self::Docstring.as_str()
    }

    pub fn is_definition_preamble(n: &Node) -> bool {
        n.kind() == Self::DefinitionPreamble.as_str()
    }

    // Identifiers and Names
    pub fn is_identifier(n: &Node) -> bool {
        n.kind() == Self::Identifier.as_str()
    }

    pub fn is_scoped_name(n: &Node) -> bool {
        n.kind() == Self::ScopedName.as_str()
    }

    pub fn is_type_name(n: &Node) -> bool {
        n.kind() == Self::TypeName.as_str()
    }

    // Type System
    pub fn is_type_parameters(n: &Node) -> bool {
        n.kind() == Self::TypeParameters.as_str()
    }

    pub fn is_type_expression(n: &Node) -> bool {
        n.kind() == Self::TypeExpression.as_str()
    }

    pub fn is_type_arguments(n: &Node) -> bool {
        n.kind() == Self::TypeArguments.as_str()
    }

    pub fn is_primitive_type(n: &Node) -> bool {
        n.kind() == Self::PrimitiveType.as_str()
    }

    // Type Definitions
    pub fn is_type_definition(n: &Node) -> bool {
        n.kind() == Self::TypeDefinition.as_str()
    }

    pub fn is_newtype_definition(n: &Node) -> bool {
        n.kind() == Self::NewtypeDefinition.as_str()
    }

    pub fn is_struct_definition(n: &Node) -> bool {
        n.kind() == Self::StructDefinition.as_str()
    }

    pub fn is_union_definition(n: &Node) -> bool {
        n.kind() == Self::UnionDefinition.as_str()
    }

    pub fn is_field(n: &Node) -> bool {
        n.kind() == Self::Field.as_str()
    }

    pub fn is_field_block(n: &Node) -> bool {
        n.kind() == Self::FieldBlock.as_str()
    }

    // Module System
    pub fn is_module_definition(n: &Node) -> bool {
        n.kind() == Self::ModuleDefinition.as_str()
    }

    pub fn is_module_body(n: &Node) -> bool {
        n.kind() == Self::ModuleBody.as_str()
    }

    pub fn is_import_declaration(n: &Node) -> bool {
        n.kind() == Self::ImportDeclaration.as_str()
    }

    pub fn is_import_path(n: &Node) -> bool {
        n.kind() == Self::ImportPath.as_str()
    }

    // Annotations
    pub fn is_annotation(n: &Node) -> bool {
        n.kind() == Self::Annotation.as_str()
    }

    pub fn is_annotations(n: &Node) -> bool {
        n.kind() == Self::Annotations.as_str()
    }

    pub fn is_annotation_declaration(n: &Node) -> bool {
        n.kind() == Self::AnnotationDeclaration.as_str()
    }

    pub fn is_field_reference(n: &Node) -> bool {
        n.kind() == Self::FieldReference.as_str()
    }

    // JSON
    pub fn is_json_value(n: &Node) -> bool {
        n.kind() == Self::JsonValue.as_str()
    }

    pub fn is_json_number(n: &Node) -> bool {
        n.kind() == Self::JsonNumber.as_str()
    }

    pub fn is_json_string(n: &Node) -> bool {
        n.kind() == Self::JsonString.as_str()
    }

    pub fn is_json_array(n: &Node) -> bool {
        n.kind() == Self::JsonArray.as_str()
    }

    pub fn is_json_object(n: &Node) -> bool {
        n.kind() == Self::JsonObject.as_str()
    }

    pub fn is_json_object_pair(n: &Node) -> bool {
        n.kind() == Self::JsonObjectPair.as_str()
    }

    // Error
    pub fn is_error(n: &Node) -> bool {
        n.kind() == Self::Error.as_str()
    }

    pub fn is_missing(n: &Node) -> bool {
        n.is_missing()
    }
}
