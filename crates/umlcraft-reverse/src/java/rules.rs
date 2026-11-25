//! How Java names resolve and which of its types are values.

use super::jdk::JDK;
use crate::lang::{Rules, split_at_capital};

pub static RULES: Rules = Rules {
    primitives: &["boolean", "byte", "char", "short", "int", "long", "float", "double", "void"],
    implicit_imports: &[&["java", "lang"]],
    library: &JDK,
    value_packages: &[&["java", "lang"]],
    // Java's naming convention: packages are lower case, types start with a capital letter.
    split_qualified: split_at_capital,
    enclosing_namespaces: false,
    base_list_by_target: false,
    optional_modifiers: &[],
    type_qualifiers: &[],
    reexports: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn split(path: &[&str]) -> usize {
        let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
        (RULES.split_qualified)(&path)
    }

    #[test]
    fn packages_end_at_the_first_capital() {
        assert_eq!(split(&["com", "acme", "Order", "Line"]), 2);
        assert_eq!(split(&["Order"]), 0);
        assert_eq!(split(&["com", "acme", "util"]), 2);
    }
}
