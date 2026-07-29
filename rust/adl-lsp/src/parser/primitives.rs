//! Registry of ADL's primitive types.
//!
//! Mirrors the canonical compiler's `Primitive.hs`: primitives are ordinary (non-user-defined)
//! type names, distinguished only by name and arity. The grammar does not treat them specially
//! (there is no `primitive_type` node kind as of grammar v0.7), so the LSP recognises them by
//! looking up the `type_expression`'s name here.

/// The 19 ADL primitives with their type-parameter arity.
pub const PRIMITIVES: [(&str, usize); 19] = [
    ("Void", 0),
    ("Bool", 0),
    ("Int8", 0),
    ("Int16", 0),
    ("Int32", 0),
    ("Int64", 0),
    ("Word8", 0),
    ("Word16", 0),
    ("Word32", 0),
    ("Word64", 0),
    ("Float", 0),
    ("Double", 0),
    ("Json", 0),
    ("Bytes", 0),
    ("String", 0),
    ("Vector", 1),
    ("StringMap", 1),
    ("Nullable", 1),
    ("TypeToken", 1),
];

/// Is `name` an ADL primitive type name?
pub fn is_primitive(name: &str) -> bool {
    PRIMITIVES.iter().any(|(n, _)| *n == name)
}

/// The type-parameter arity of the primitive `name`, or `None` if it is not a primitive.
#[allow(dead_code)]
pub fn primitive_arity(name: &str) -> Option<usize> {
    PRIMITIVES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, arity)| *arity)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn recognises_primitives_and_arity() {
        assert!(is_primitive("String"));
        assert!(is_primitive("Vector"));
        assert!(!is_primitive("MyStruct"));
        assert_eq!(primitive_arity("Void"), Some(0));
        assert_eq!(primitive_arity("StringMap"), Some(1));
        assert_eq!(primitive_arity("Person"), None);
        assert_eq!(PRIMITIVES.len(), 19);
    }
}
