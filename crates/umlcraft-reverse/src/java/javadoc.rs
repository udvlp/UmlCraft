//! Block and Javadoc comments as documentation text.

/// The text of a `/** … */` or `/* … */` comment: without the delimiters, and on every line
/// without the leading whitespace, stars and the blanks after them (Astah removes `\**[ \t]*`
/// at line starts). Blank lines at the start and end are dropped.
pub fn clean(comment: &str) -> String {
    let inner = comment.strip_prefix("/*").unwrap_or(comment);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    let lines: Vec<&str> = inner
        .lines()
        .map(|line| {
            line.trim_start().trim_start_matches('*').trim_start_matches([' ', '\t']).trim_end()
        })
        .collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_delimiters_stars_and_blank_edges() {
        let comment = "/**\n   * Holds an order.\n   *\n   *   @author Ulf\n   */";
        assert_eq!(clean(comment), "Holds an order.\n\n@author Ulf");
        assert_eq!(clean("/** One line. */"), "One line.");
        assert_eq!(clean("/***/"), "");
        assert_eq!(clean("/**/"), "");
        assert_eq!(clean("/* Plain. */"), "Plain.");
    }
}
