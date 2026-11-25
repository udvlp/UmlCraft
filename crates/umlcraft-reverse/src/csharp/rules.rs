//! How C# names resolve and which of its types are values.

use crate::lang::{Rules, split_at_last};
use crate::library::TableLibrary;

/// The containers and value types of the .NET base class library, kept by hand in `bcl.txt`.
pub static BCL: TableLibrary = TableLibrary::new(include_str!("bcl.txt"));

pub static RULES: Rules = Rules {
    primitives: &[
        "bool", "byte", "sbyte", "char", "decimal", "double", "float", "int", "uint", "nint",
        "nuint", "long", "ulong", "short", "ushort", "object", "string", "void", "dynamic",
    ],
    // What the .NET SDK's implicit usings bring that the table knows.
    implicit_imports: &[&["System"], &["System", "Collections", "Generic"]],
    library: &BCL,
    value_packages: &[&["System"]],
    // Namespaces are capitalized like types.
    split_qualified: split_at_last,
    enclosing_namespaces: true,
    base_list_by_target: true,
    optional_modifiers: &["?"],
    type_qualifiers: &[],
    reexports: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::TypeKind;
    use crate::lang::Container;

    fn s(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_collections_and_values() {
        let generic = s(&["System", "Collections", "Generic"]);
        let list = BCL.get(&generic, &s(&["List"])).unwrap();
        assert_eq!(list.kind, TypeKind::Class);
        assert_eq!(list.container, Container::Collection { ordered: true, unique: false });
        let map = BCL.get(&generic, &s(&["IDictionary"])).unwrap();
        assert_eq!((map.kind, map.container), (TypeKind::Interface, Container::Map));
        assert_eq!(map.parameters, ["TKey", "TValue"]);
        let nullable = BCL.get(&s(&["System"]), &s(&["Nullable"])).unwrap();
        assert_eq!(nullable.container, Container::Optional);
        assert!(BCL.is_package(&s(&["System", "Collections"])));
        assert_eq!((RULES.split_qualified)(&s(&["Acme", "Shop", "Order"])), 2);
    }
}
