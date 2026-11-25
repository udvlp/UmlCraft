//! How Python names resolve and which of its types are values.

use crate::lang::{Rules, split_at_capital};
use crate::library::TableLibrary;

/// The containers and value types of the standard library, kept by hand in `stdlib.txt`.
pub static STDLIB: TableLibrary = TableLibrary::new(include_str!("stdlib.txt"));

pub static RULES: Rules = Rules {
    primitives: &["int", "float", "complex", "str", "bytes", "bool", "object", "None"],
    implicit_imports: &[&["builtins"]],
    library: &STDLIB,
    // `typing` and `collections` hold containers, whose elements count, and helpers such as
    // `Any` that are no association ends.
    value_packages: &[
        &["builtins"],
        &["typing"],
        &["collections"],
        &["collections", "abc"],
        &["datetime"],
        &["decimal"],
        &["uuid"],
        &["pathlib"],
        &["enum"],
    ],
    // PEP 8: modules are lower case, classes start with a capital letter (Python names have no
    // `$`).
    split_qualified: split_at_capital,
    enclosing_namespaces: false,
    base_list_by_target: false,
    optional_modifiers: &[" | None"],
    type_qualifiers: &[],
    reexports: true,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Container;

    fn s(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_containers() {
        let list = STDLIB.get(&s(&["builtins"]), &s(&["list"])).unwrap();
        assert_eq!(list.container, Container::Collection { ordered: true, unique: false });
        let optional = STDLIB.get(&s(&["typing"]), &s(&["Optional"])).unwrap();
        assert_eq!(optional.container, Container::Optional);
        assert!(STDLIB.is_package(&s(&["collections", "abc"])));
        assert_eq!((RULES.split_qualified)(&s(&["shop", "orders", "Order", "Line"])), 2);
    }
}
