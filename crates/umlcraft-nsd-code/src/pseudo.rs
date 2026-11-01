//! Code in the notation of structograms: no types, assignments as `x := value`, no `;`,
//! conditions as questions (`a > 0?`), values handed back as `return x`, and loops as
//! `for i := a to b by s` and `foreach x in c`, or with the diagram's German keywords
//! `gib x`, `für i := a bis b Schritt s` and `für jedes x in c`. Java and C# share the shapes of the
//! nodes used here (assignments, binary expressions, for loops).

use tree_sitter::Node;
use umlcraft_nsd::Keywords;

use crate::text::{lines_at, node_text, one_line};

/// A condition as a question: `a > 0` → `a > 0?`.
pub fn question(mut text: Vec<String>) -> Vec<String> {
    if let Some(last) = text.last_mut()
        && !last.is_empty()
    {
        last.push('?');
    }
    text
}

/// A value handed back: `x` → `return x` (`gib x`).
pub fn give(keywords: Keywords, mut value: Vec<String>) -> Vec<String> {
    let keyword = keywords.word("preReturn");
    match value.first_mut() {
        Some(first) => first.insert_str(0, &format!("{keyword} ")),
        None => value.push(keyword.to_owned()),
    }
    value
}

/// The text of a counting loop: `for i := a to b by s` (`für i := a bis b Schritt s`), without
/// the step when it is 1.
pub fn counter(keywords: Keywords, var: &str, start: &str, end: &str, step: i64) -> String {
    let [pre, post] = [keywords.word("preFor"), keywords.word("postFor")];
    match step {
        1 => format!("{pre} {var} := {start} {post} {end}"),
        _ => format!("{pre} {var} := {start} {post} {end} {} {step}", keywords.word("stepFor")),
    }
}

/// `name := value`, the value's further lines as they are.
pub fn assign(name: &str, mut value: Vec<String>) -> Vec<String> {
    match value.first_mut() {
        Some(first) => first.insert_str(0, &format!("{name} := ")),
        None => value.push(format!("{name} :=")),
    }
    value
}

/// An assignment or increment as `x := value`: `x = 1` → `x := 1`, `total += a + b` →
/// `total := total + (a + b)`, `i++` → `i := i + 1`, C#'s `x ??= y` → `x := x ?? y` and
/// `var (a, b) = p` → `(a, b) := p`. Other expressions give `None`.
pub fn assignment(expr: Node, src: &str) -> Option<Vec<String>> {
    match expr.kind() {
        "assignment_expression" | "augmented_assignment" => {
            let left = expr.child_by_field_name("left")?;
            let left = match left.child_by_field_name("type") {
                // `var (a, b)`: what is declared, without its type.
                Some(t) if left.kind() == "declaration_expression" => {
                    src[t.end_byte()..left.end_byte()].trim()
                }
                _ => node_text(left, src),
            };
            let op = node_text(expr.child_by_field_name("operator")?, src);
            let right = expr.child_by_field_name("right")?;
            let column = expr.start_position().column;
            let value = match op.strip_suffix('=').filter(|o| !o.is_empty()) {
                None => lines_at(right, src, column),
                Some(op) => {
                    let mut value = operand(right, src, column, precedence(op));
                    value[0].insert_str(0, &format!("{left} {op} "));
                    value
                }
            };
            Some(assign(left, value))
        }
        "update_expression" | "postfix_unary_expression" | "prefix_unary_expression" => {
            let operand = expr.named_child(0)?;
            let op = increment(expr, src)?;
            let name = one_line(node_text(operand, src));
            Some(vec![format!("{name} := {name} {} 1", if op > 0 { "+" } else { "-" })])
        }
        _ => None,
    }
}

/// `for i := a to b by s` for a loop that counts `var` from `start` by a constant step:
/// `for (int i = 0; i < n; i++)` → `for i := 0 to n - 1`. Other for loops give `None`.
pub fn counting(
    keywords: Keywords,
    node: Node,
    var: Node,
    start: Node,
    src: &str,
) -> Option<String> {
    let var_node = var;
    let var = node_text(var, src);

    let condition = node.child_by_field_name("condition")?;
    let op = node_text(condition.child_by_field_name("operator")?, src);
    let left = condition.child_by_field_name("left")?;
    let bound = condition.child_by_field_name("right")?;
    if condition.kind() != "binary_expression" || node_text(left, src) != var {
        return None;
    }
    // `to n - 1` counts whole numbers only: `x < 2.5` would end a pass too early.
    let declared = std::iter::successors(var_node.parent(), |p| p.parent())
        .take_while(|p| *p != node)
        .filter_map(|p| p.child_by_field_name("type"))
        .any(|t| ["float", "double", "decimal"].iter().any(|f| node_text(t, src).contains(f)));
    if declared || fraction(start, src) || fraction(bound, src) {
        return None;
    }

    let mut cursor = node.walk();
    let updates: Vec<Node> =
        node.children_by_field_name("update", &mut cursor).filter(|n| n.is_named()).collect();
    let [update] = updates.as_slice() else { return None };
    let step = step(*update, var, src)?;

    let end = match (op, step > 0) {
        ("<", true) => offset(bound, src, -1),
        (">", false) => offset(bound, src, 1),
        ("<=", true) | (">=", false) => one_line(node_text(bound, src)),
        _ => return None,
    };
    let start = one_line(node_text(start, src));
    Some(counter(keywords, var, &start, &end, step))
}

/// Whether an expression holds a number with a fraction (`2.5`, `1e3`, `0.5f`).
fn fraction(expr: Node, src: &str) -> bool {
    let literal = match expr.kind() {
        "decimal_floating_point_literal" | "hex_floating_point_literal" | "real_literal" => true,
        "number_literal" => {
            let text = node_text(expr, src);
            !text.starts_with("0x") && !text.starts_with("0X") && text.contains(['.', 'e', 'E'])
        }
        _ => false,
    };
    let mut cursor = expr.walk();
    literal || expr.named_children(&mut cursor).any(|c| fraction(c, src))
}

/// `foreach v in values` (`für jedes v in values`) for `for (T v : values)`.
pub fn traversal(keywords: Keywords, name: &str, mut value: Vec<String>) -> Vec<String> {
    let head = format!("{} {name} {}", keywords.word("preForIn"), keywords.word("postForIn"));
    match value.first_mut() {
        Some(first) => first.insert_str(0, &format!("{head} ")),
        None => value.push(head),
    }
    value
}

/// The constant a counting loop's update adds to `var`: `i++` 1, `i -= 2` -2.
fn step(update: Node, var: &str, src: &str) -> Option<i64> {
    match update.kind() {
        "update_expression" | "postfix_unary_expression" | "prefix_unary_expression" => {
            let operand = update.named_child(0)?;
            if node_text(operand, src) != var {
                return None;
            }
            increment(update, src)
        }
        "assignment_expression" => {
            if node_text(update.child_by_field_name("left")?, src) != var {
                return None;
            }
            let right = update.child_by_field_name("right")?;
            let n: i64 = node_text(right, src).parse().ok().filter(|&n| n > 0)?;
            match node_text(update.child_by_field_name("operator")?, src) {
                "+=" => Some(n),
                "-=" => Some(-n),
                _ => None,
            }
        }
        _ => None,
    }
}

/// 1 for `i++`/`++i`, -1 for `i--`/`--i`, `None` for other unary expressions (`-x`, `!b`).
fn increment(expr: Node, src: &str) -> Option<i64> {
    let operand = expr.named_child(0)?;
    let text = node_text(expr, src);
    let op = text.replacen(node_text(operand, src), "", 1);
    match op.trim() {
        "++" => Some(1),
        "--" => Some(-1),
        _ => None,
    }
}

/// `bound + by` on one line: a number is computed, an expression gets `- 1` or `+ 1`.
pub fn offset(bound: Node, src: &str, by: i64) -> String {
    if let Ok(n) = node_text(bound, src).parse::<i64>() {
        return (n + by).to_string();
    }
    // As the left operand of `-`/`+`, a sum needs no parentheses.
    let text = operand(bound, src, 0, precedence("+") - 1).join(" ");
    let text = one_line(&text);
    if by < 0 { format!("{text} - {}", -by) } else { format!("{text} + {by}") }
}

/// An expression's lines (see [`lines_at`]) as the operand of an operator of `outer`
/// precedence, in parentheses where it would otherwise bind differently.
fn operand(expr: Node, src: &str, column: usize, outer: u8) -> Vec<String> {
    let inner = match expr.kind() {
        "binary_expression" | "binary_operator" | "boolean_operator" => {
            expr.child_by_field_name("operator").map_or(0, |o| precedence(node_text(o, src)))
        }
        "comparison_operator" => precedence("<"),
        "not_operator" => precedence("and"),
        "instanceof_expression" | "is_expression" | "as_expression" | "is_pattern_expression" => {
            precedence("<")
        }
        "ternary_expression"
        | "conditional_expression"
        | "assignment_expression"
        | "lambda_expression"
        | "lambda"
        | "comma_expression"
        | "switch_expression" => 0,
        _ => u8::MAX,
    };
    let mut text = lines_at(expr, src, column);
    if inner <= outer {
        text[0].insert(0, '(');
        if let Some(last) = text.last_mut() {
            last.push(')');
        }
    }
    text
}

/// How tightly a binary operator binds (Java's, C#'s and Python's order).
fn precedence(op: &str) -> u8 {
    match op {
        "**" => 13,
        "*" | "/" | "%" | "//" | "@" => 12,
        "+" | "-" => 11,
        "<<" | ">>" | ">>>" => 10,
        "<" | ">" | "<=" | ">=" => 9,
        "==" | "!=" => 8,
        "&" => 7,
        "^" => 6,
        "|" => 5,
        "&&" | "and" => 4,
        "||" | "or" => 3,
        "??" => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Source, SourceLanguage};

    /// The first statement of `body` as the notation gives it.
    fn first(body: &str) -> Vec<String> {
        written(body, umlcraft_nsd::Keywords::English).children.elements[0].text()
    }

    /// The diagram of a method with `body`, its keywords in the language of `keywords`.
    fn written(body: &str, keywords: umlcraft_nsd::Keywords) -> umlcraft_nsd::Diagram {
        let source =
            Source::parse(SourceLanguage::Java, format!("class A {{ int f(int n) {{ {body} }} }}"));
        let m = &source.methods()[0];
        let made = crate::Made { author: "t", date: "2026-10-30", origin: "test", keywords };
        source.diagram(m, &made).unwrap().0
    }

    #[test]
    fn german_keywords_count_and_give_in_german() {
        let german = umlcraft_nsd::Keywords::German;
        let d = written(
            "for (int i = 1; i <= n; i += 2) { } for (int v : values) { } return n;",
            german,
        );
        assert_eq!(umlcraft_nsd::Keywords::of(&d), german);
        let texts: Vec<Vec<String>> = d.children.elements.iter().map(|e| e.text()).collect();
        assert_eq!(
            texts,
            [vec!["für i := 1 bis n Schritt 2"], vec!["für jedes v in values"], vec!["gib n"]]
        );
        let styles: Vec<_> = d.children.elements[..2].iter().map(|e| e.attr("style")).collect();
        assert_eq!(styles, [Some("COUNTER"), Some("TRAVERSAL")]);
        assert_eq!(first("return n;"), ["return n"]);
    }

    #[test]
    fn assignments_lose_types_and_semicolons() {
        assert_eq!(first("int y = n + 1;"), ["y := n + 1"]);
        assert_eq!(first("var s = \"x\";"), ["s := \"x\""]);
        assert_eq!(first("n = 2;"), ["n := 2"]);
        assert_eq!(first("n++;"), ["n := n + 1"]);
        assert_eq!(first("--n;"), ["n := n - 1"]);
        assert_eq!(first("n += 3;"), ["n := n + 3"]);
        assert_eq!(first("n -= a - b;"), ["n := n - (a - b)"]);
        assert_eq!(first("n *= a + b;"), ["n := n * (a + b)"]);
        assert_eq!(first("n += a * b;"), ["n := n + a * b"]);
        assert_eq!(first("a[n] <<= 1;"), ["a[n] := a[n] << 1"]);
        assert_eq!(first("list.clear();"), ["list.clear()"]);
    }

    #[test]
    fn values_over_several_lines_keep_their_indentation() {
        let body =
            "\n        Runnable r = new Runnable() {\n            void run() { }\n        };\n";
        assert_eq!(first(body), ["r := new Runnable() {", "    void run() { }", "}"]);
    }

    #[test]
    fn declarations_without_a_value_vanish() {
        assert_eq!(first("int x; int a = 1, b, c = 2;"), ["a := 1"]);
    }

    #[test]
    fn counting_loops() {
        assert_eq!(first("for (int i = 0; i < n; i++) { }"), ["for i := 0 to n - 1"]);
        assert_eq!(first("for (int i = 0; i < 10; i++) { }"), ["for i := 0 to 9"]);
        assert_eq!(first("for (int i = 0; i < n + 1; i++) { }"), ["for i := 0 to n + 1 - 1"]);
        assert_eq!(first("for (int i = 1; i <= n; i += 2) { }"), ["for i := 1 to n by 2"]);
        assert_eq!(first("for (int i = n; i > 0; i--) { }"), ["for i := n to 1 by -1"]);
        assert_eq!(first("for (i = n; i >= a; i -= 3) { }"), ["for i := n to a by -3"]);
        assert_eq!(first("for (int i = 0; i < n << 1; i++) { }"), ["for i := 0 to (n << 1) - 1"]);
        assert_eq!(first("for (int v : values) { }"), ["foreach v in values"]);
    }

    #[test]
    fn other_for_loops_become_while_loops() {
        // Wrong direction, a step that is no constant, two variables.
        assert_eq!(first("for (int i = 0; i < n; i--) { }"), ["i := 0"]);
        assert_eq!(first("for (int i = 0; i < n; i += k) { }"), ["i := 0"]);
        assert_eq!(first("for (int i = 0, j = n; i < j; i++, j--) { }"), ["i := 0"]);
    }

    #[test]
    fn signatures_name_only_the_parameters() {
        let source = Source::parse(
            SourceLanguage::Java,
            "class A { <T> int f(final java.util.List<T> xs, int[] a, String... rest) { return 0; }
                       void g(A this, int x) { } record P(int x) { P { } } }",
        );
        let made = crate::Made {
            author: "t",
            date: "2026-10-30",
            origin: "test",
            keywords: umlcraft_nsd::Keywords::English,
        };
        let texts: Vec<Vec<String>> =
            source.methods().iter().map(|m| source.diagram(m, &made).unwrap().0.text()).collect();
        assert_eq!(texts, [vec!["f(xs, a, rest)"], vec!["g(x)"], vec!["P"]]);
    }
}
