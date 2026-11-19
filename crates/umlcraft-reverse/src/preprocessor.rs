//! Conditional compilation (C#, C++): of `#if` … `#elif` … `#else`, only the first branch is
//! read; nothing is evaluated, except that a condition that is a constant false (`#if 0`,
//! `#if false`, `#elif 0`) leaves its branch out, so the next one is read: `#if 0` is how code
//! is switched off. Include guards (`#ifndef X_H`) keep the whole file.

/// The text as the first branches of its conditionals read it: the lines of `#if`, `#ifdef`,
/// `#ifndef`, `#elif`, `#elifdef`, `#elifndef`, `#else` and `#endif`, and the branches after
/// the first, blanked out, with where each branch left out starts (the byte its directive's
/// line starts at). A branch under a constant false condition (`#if 0`) is left out and does
/// not count as the first. Blanking keeps every byte's place, so lines and columns stay as
/// written, and declarations that the conditionals cut apart parse as one. In C++ a blanked
/// line that ends in `\` blanks the next one too; C# has no line continuation. Lines inside
/// block comments and multi-line strings are no directives.
pub fn first_branches(text: &str, dialect: Dialect) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    // Per open `#if`: whether a branch was read already, and whether the branch now is left
    // out.
    let mut open: Vec<Open> = Vec::new();
    let mut branches = Vec::new();
    // Whether the line before was blanked and goes on in this one (`#if A || \`).
    let mut continued = false;
    let mut state = Lexical::Code;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let in_code = state == Lexical::Code;
        state = scan(line, state, dialect);
        let directive = line.trim_start().strip_prefix('#').map(str::trim_start);
        let directive = directive.filter(|_| !continued && in_code);
        let word = directive.map(|d| d.split(|c: char| !c.is_ascii_alphabetic()).next());
        let left_out = |open: &[Open]| open.iter().any(|o| o.left_out);
        let blank = match (word.flatten(), directive) {
            (Some(word @ ("if" | "ifdef" | "ifndef")), Some(directive)) => {
                let off = word == "if" && constant_false(&directive[word.len()..]);
                if off && !left_out(&open) {
                    branches.push(start);
                }
                open.push(Open { read: !off, left_out: off });
                true
            }
            (Some(word @ ("elif" | "elifdef" | "elifndef" | "else")), Some(directive)) => {
                let off = word == "elif" && constant_false(&directive[word.len()..]);
                let outer = open.len().saturating_sub(1);
                let outer_left_out = left_out(&open[..outer]);
                if let Some(last) = open.last_mut() {
                    last.left_out = last.read || off;
                    last.read |= !last.left_out;
                    if last.left_out && !outer_left_out {
                        branches.push(start);
                    }
                }
                true
            }
            (Some("endif"), _) => {
                open.pop();
                true
            }
            _ => continued || left_out(&open),
        };
        continued = dialect == Dialect::Cpp && blank && line.trim_end().ends_with('\\');
        if blank {
            for c in line.chars() {
                match c {
                    '\r' | '\n' => out.push(c),
                    _ => out.extend(std::iter::repeat_n(' ', c.len_utf8())),
                }
            }
        } else {
            out.push_str(line);
        }
    }
    (out, branches)
}

/// An open conditional.
struct Open {
    /// Whether one of its branches was read.
    read: bool,
    /// Whether the branch now is left out.
    left_out: bool,
}

/// Whether the condition of an `#if` or `#elif` (what follows the word) is a constant false:
/// `0`, `false`, also in parentheses and with a comment after it.
fn constant_false(condition: &str) -> bool {
    let condition = condition.split("//").next().unwrap_or_default();
    let condition = condition.split("/*").next().unwrap_or_default();
    let mut condition = condition.trim();
    while let Some(inner) = condition.strip_prefix('(').and_then(|c| c.strip_suffix(')')) {
        condition = inner.trim();
    }
    matches!(condition, "0" | "false")
}

/// The language whose preprocessor rules apply.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Cpp,
    CSharp,
}

/// What a line ends in, as far as it goes on into the next one.
#[derive(Clone, PartialEq, Eq)]
enum Lexical {
    Code,
    /// `/* … */`
    Comment,
    /// C#'s `@"…"`, where `""` is a quote.
    Verbatim,
    /// C#'s raw string, closed by as many quotes as opened it.
    Raw(usize),
    /// C++'s `R"x(…)x"`, closed by `)x"`.
    CppRaw(String),
}

/// The state at the end of `line`, read from `state`. Ordinary strings and character literals
/// end on their line; line comments end the line.
fn scan(line: &str, mut state: Lexical, dialect: Dialect) -> Lexical {
    let b = line.as_bytes();
    let mut i = 0;
    let quotes = |i: usize| b[i..].iter().take_while(|&&c| c == b'"').count();
    let cs = dialect == Dialect::CSharp;
    while i < b.len() {
        match &state {
            Lexical::Comment => match find(&b[i..], b"*/") {
                Some(at) => (state, i) = (Lexical::Code, i + at + 2),
                None => return state,
            },
            Lexical::Verbatim => match b[i] {
                b'"' if b.get(i + 1) == Some(&b'"') => i += 2,
                b'"' => (state, i) = (Lexical::Code, i + 1),
                _ => i += 1,
            },
            Lexical::Raw(n) => {
                let run = quotes(i);
                if run >= *n {
                    (state, i) = (Lexical::Code, i + run);
                } else {
                    i += run.max(1);
                }
            }
            Lexical::CppRaw(end) => match find(&b[i..], end.as_bytes()) {
                Some(at) => (state, i) = (Lexical::Code, i + at + end.len()),
                None => return state,
            },
            Lexical::Code => {
                // Bytes, as a step may land inside a character.
                let rest = &b[i..];
                let word_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
                if rest.starts_with(b"//") {
                    return state;
                } else if rest.starts_with(b"/*") {
                    (state, i) = (Lexical::Comment, i + 2);
                } else if cs && (rest.starts_with(b"@\"")) {
                    (state, i) = (Lexical::Verbatim, i + 2);
                } else if cs && (rest.starts_with(b"@$\"") || rest.starts_with(b"$@\"")) {
                    (state, i) = (Lexical::Verbatim, i + 3);
                } else if cs && quotes(i) >= 3 {
                    let n = quotes(i);
                    (state, i) = (Lexical::Raw(n), i + n);
                } else if !cs && rest.starts_with(b"R\"") && cpp_raw_prefix(&b[..i]) {
                    match rest[2..].iter().position(|&c| c == b'(') {
                        Some(at) => {
                            let delimiter = String::from_utf8_lossy(&rest[2..2 + at]);
                            let end = format!("){delimiter}\"");
                            (state, i) = (Lexical::CppRaw(end), i + 2 + at + 1);
                        }
                        None => i += 2,
                    }
                } else if b[i] == b'"' {
                    i = end_of_literal(b, i, b'"');
                } else if b[i] == b'\'' && !word_before {
                    // A character literal, not a digit separator (`1'000`).
                    i = end_of_literal(b, i, b'\'');
                } else {
                    i += 1;
                }
            }
        }
    }
    state
}

/// Whether the text before an `R"` makes it a raw string: nothing or an encoding prefix
/// (`u8R"`, `LR"`), not the end of a name.
fn cpp_raw_prefix(before: &[u8]) -> bool {
    let start =
        before.iter().rposition(|&c| !c.is_ascii_alphanumeric() && c != b'_').map_or(0, |p| p + 1);
    matches!(&before[start..], b"" | b"u8" | b"u" | b"U" | b"L")
}

/// Where `pattern` starts in `b`.
fn find(b: &[u8], pattern: &[u8]) -> Option<usize> {
    b.windows(pattern.len()).position(|w| w == pattern)
}

/// Just past the literal opened by `quote` at `start`, or the end of the line.
fn end_of_literal(b: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == quote => return i + 1,
            b'\n' => return i,
            _ => i += 1,
        }
    }
    b.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_first_branches() {
        let text = "a\n#if A\nb\n  #if B\nc\n  #else\nd\n  #endif\n#elif C\ne\n#if D\nf\n#else\ng\n#endif\n#else\nh\n#endif\ni\n";
        let (kept, branches) = first_branches(text, Dialect::Cpp);
        let lines: Vec<&str> = kept.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["a", "b", "c", "i"]);
        assert_eq!(kept.len(), text.len());
        // `#else` of B, `#elif C` and `#else` of A; the `#else` of D is inside a branch left out.
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn blanks_continued_directives() {
        let text = "#if defined(A) || \\\n    defined(B)\na\n#else\nb \\\nc\n#endif\nd\n";
        let (kept, branches) = first_branches(text, Dialect::Cpp);
        let lines: Vec<&str> = kept.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["a", "d"]);
        assert_eq!(branches.len(), 1);
    }

    #[test]
    fn continues_lines_only_in_cpp() {
        let text = "#if A\n// C:\\temp\\\n#endif\nclass A {}\n";
        let (kept, _) = first_branches(text, Dialect::CSharp);
        let lines: Vec<&str> = kept.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["// C:\\temp\\", "class A {}"]);
    }

    #[test]
    fn strings_and_comments_hold_no_directives() {
        let cs = "var a = @\"x\n#if A\n\"; var b = \"\"\"\n#else\n\"\"\";\n/* \n#endif\n*/ var c = '\"';\n#if B\nd\n#endif\n";
        let (kept, branches) = first_branches(cs, Dialect::CSharp);
        assert!(kept.contains("#if A") && kept.contains("#else") && kept.contains("#endif\n*/"));
        assert!(kept.contains("d\n") && branches.is_empty());
        let cpp = "auto s = R\"x(\n#if A\n)x\"; auto t = u8R\"(\n#endif\n)\";\nint n = 1'000;\n#if B\nb\n#else\nc\n#endif\n";
        let (kept, branches) = first_branches(cpp, Dialect::Cpp);
        assert!(kept.contains("#if A") && kept.contains("\n#endif\n)"));
        assert!(kept.contains("b\n") && !kept.contains("c\n") && branches.len() == 1);
    }

    #[test]
    fn knows_the_c_directives() {
        let text = "#ifndef A_H\n#define A_H\na\n#ifdef B\nb\n#elifdef C\nc\n#endif\n#endif\n";
        let (kept, branches) = first_branches(text, Dialect::Cpp);
        let lines: Vec<&str> = kept.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["#define A_H", "a", "b"]);
        assert_eq!(branches.len(), 1);
    }

    #[test]
    fn a_constant_false_condition_reads_the_next_branch() {
        let read = |text: &str, dialect| {
            let (kept, branches) = first_branches(text, dialect);
            let lines: Vec<String> =
                kept.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
            (lines, branches)
        };
        let text = "a\n#if 0 // off\nold(\n#else\nnew\n#endif\nb\n";
        let (lines, branches) = read(text, Dialect::Cpp);
        assert_eq!(lines, ["a", "new", "b"]);
        // Where the branch left out starts: the line of `#if 0`.
        assert_eq!(branches, [2]);
        let (lines, branches) = read("#if (false)\nx\n#endif\ny\n", Dialect::CSharp);
        assert_eq!((lines, branches.len()), (vec!["y".to_owned()], 1));
        // `#elif 0` is left out too; the branch after it is read when none was before.
        let text = "#if 0\na\n#elif 0\nb\n#elif C\nc\n#else\nd\n#endif\n";
        let (lines, branches) = read(text, Dialect::Cpp);
        assert_eq!((lines, branches.len()), (vec!["c".to_owned()], 3));
        // Inside a branch left out nothing counts; `#if 01` or `#if 0 + 1` is no constant false.
        let text = "#if A\n#else\n#if 0\nx\n#endif\n#endif\n#if 0 + 1\ny\n#endif\n";
        let (lines, branches) = read(text, Dialect::Cpp);
        assert_eq!((lines, branches.len()), (vec!["y".to_owned()], 1));
    }
}
