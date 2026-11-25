//! How C++ names resolve and which of its types are values.

use crate::lang::{Rules, split_at_last};
use crate::library::TableLibrary;

/// The containers, smart pointers and value types of the standard library, kept by hand in
/// `std.txt`.
pub static STD: TableLibrary = TableLibrary::new(include_str!("std.txt"));

pub static RULES: Rules = Rules {
    primitives: &[
        "void", "bool", "char", "wchar_t", "char8_t", "char16_t", "char32_t", "short", "int",
        "long", "float", "double", "signed", "unsigned", "auto", "size_t",
    ],
    implicit_imports: &[],
    library: &STD,
    value_packages: &[&["std"]],
    // Namespaces and types look alike.
    split_qualified: split_at_last,
    enclosing_namespaces: true,
    base_list_by_target: false,
    // A pointer may be null; a reference may not.
    optional_modifiers: &["*"],
    type_qualifiers: &["const ", "volatile "],
    reexports: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Container;

    fn s(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_containers_and_smart_pointers() {
        let std = s(&["std"]);
        let vector = STD.get(&std, &s(&["vector"])).unwrap();
        assert_eq!(vector.container, Container::Collection { ordered: true, unique: false });
        assert_eq!(STD.get(&std, &s(&["map"])).unwrap().parameters, ["K", "V"]);
        assert_eq!(STD.get(&std, &s(&["unique_ptr"])).unwrap().container, Container::Owning);
        assert_eq!(STD.get(&std, &s(&["string"])).unwrap().container, Container::None);
        assert!(STD.is_package(&std));
        assert_eq!((RULES.split_qualified)(&s(&["shop", "orders", "Order"])), 2);
    }
}
