//! How Java names resolve and which of its types are values.

use super::jdk::Jdk;
use crate::lang::Rules;

pub static RULES: Rules = Rules {
    primitives: &["boolean", "byte", "char", "short", "int", "long", "float", "double", "void"],
    implicit_imports: &[&["java", "lang"]],
    library: &Jdk,
    value_packages: &[&["java", "lang"]],
    split_qualified,
};

/// Java's naming convention: packages are lower case, types start with a capital letter. The
/// package ends before the first segment that starts with a capital letter or contains a `$`.
fn split_qualified(path: &[String]) -> usize {
    path.iter()
        .position(|s| s.starts_with(|c: char| c.is_uppercase()) || s.contains('$'))
        .unwrap_or(path.len().saturating_sub(1))
}

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
