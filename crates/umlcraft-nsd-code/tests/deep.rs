//! Bodies that nest deeply: a structogram nests at most `umlcraft_nsd::read::MAX_DEPTH` deep
//! (what a file holds); a deeper method gives none, and pathologically deep code (thousands of
//! blocks, a 5,000-branch `else if` chain) neither overflows the stack nor takes long. Each
//! test runs on a thread with 1 MB of stack.

use umlcraft_nsd::read::MAX_DEPTH;
use umlcraft_nsd_code::{Made, Source, SourceLanguage, Unbuilt};

const MADE: Made = Made {
    author: "tester",
    date: "2026-11-18",
    origin: "UmlCraft test",
    keywords: umlcraft_nsd::Keywords::English,
};

/// Runs `test` on a thread with 1 MB of stack.
fn small_stack<T: Send + 'static>(test: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(1 << 20).spawn(test).unwrap().join().unwrap()
}

/// The diagram of the only method in `text`.
fn build(language: SourceLanguage, text: String) -> Result<umlcraft_nsd::Diagram, Unbuilt> {
    let source = Source::parse(language, text);
    let methods = source.methods();
    assert_eq!(methods.len(), 1, "{language:?}");
    source.diagram(&methods[0], &MADE).map(|(d, _)| d)
}

/// A method whose body is an `if` … `else if` chain of `n` conditions.
fn chain(language: SourceLanguage, n: usize) -> String {
    match language {
        SourceLanguage::Python => {
            let mut s = String::from("def f(x):\n    if x == 0:\n        a()\n");
            for i in 1..n {
                s.push_str(&format!("    elif x == {i}:\n        a()\n"));
            }
            s
        }
        _ => {
            let mut s = String::from("if (x == 0) a();\n");
            for i in 1..n {
                s.push_str(&format!("else if (x == {i}) a();\n"));
            }
            method(language, &s)
        }
    }
}

/// A method whose body is `n` blocks one inside the other (Python: `n` nested `if`s).
fn blocks(language: SourceLanguage, n: usize) -> String {
    match language {
        SourceLanguage::Python => {
            let mut s = String::from("def f(x):\n");
            for i in 0..n {
                s.push_str(&" ".repeat(i + 4));
                s.push_str("if x:\n");
            }
            s.push_str(&" ".repeat(n + 4));
            s.push_str("a()\n");
            s
        }
        _ => method(language, &format!("{}a();{}", "{".repeat(n), "}".repeat(n))),
    }
}

fn method(language: SourceLanguage, body: &str) -> String {
    match language {
        SourceLanguage::Cpp => format!("void f(int x) {{\n{body}\n}}\n"),
        _ => format!("class A {{\n  void f(int x) {{\n{body}\n  }}\n}}\n"),
    }
}

#[test]
fn an_else_if_chain_as_deep_as_a_file_holds_is_built_and_reads_back() {
    let built = small_stack(|| {
        SourceLanguage::ALL.map(|language| {
            // The calls in the last condition's branch are one level deeper than it.
            let d = build(language, chain(language, MAX_DEPTH - 1))
                .unwrap_or_else(|e| panic!("{language:?}: {e:?}"));
            assert_eq!(d.depth(), MAX_DEPTH, "{language:?}");
            let deeper = build(language, chain(language, MAX_DEPTH));
            assert_eq!(deeper, Err(Unbuilt::TooDeep), "{language:?}");
            d
        })
    });
    // Reading a file back is not the builder's: on the test's own stack.
    for d in built {
        assert_eq!(umlcraft_nsd::read(&umlcraft_nsd::write(&d)).unwrap(), d);
    }
}

#[test]
fn pathologically_deep_bodies_give_no_structogram() {
    small_stack(|| {
        let start = std::time::Instant::now();
        for language in SourceLanguage::ALL {
            assert_eq!(build(language, chain(language, 5_000)), Err(Unbuilt::TooDeep));
            let n = if language == SourceLanguage::Python { 1_000 } else { 5_000 };
            assert_eq!(build(language, blocks(language, n)), Err(Unbuilt::TooDeep), "{language:?}");
        }
        assert!(start.elapsed().as_secs() < 30, "took {:?}", start.elapsed());
    });
}

/// Code that nests without making the structogram deep: blocks in a `switch` case, a long
/// comma expression, a long condition.
#[test]
fn deep_code_inside_one_element_is_no_problem() {
    small_stack(|| {
        let cases = method(
            SourceLanguage::Java,
            &format!("switch (x) {{ case 1: {}a();{} }}", "{".repeat(5_000), "}".repeat(5_000)),
        );
        let d = build(SourceLanguage::Java, cases).unwrap();
        assert!(d.depth() <= 2);
        let commas: Vec<String> = (0..5_000).map(|i| format!("i{i}++")).collect();
        let comma = method(SourceLanguage::Cpp, &format!("{};", commas.join(", ")));
        let d = build(SourceLanguage::Cpp, comma).unwrap();
        assert_eq!(d.children.elements.len(), 5_000);
        let terms: Vec<String> = (0..5_000).map(|i| format!("i{i}")).collect();
        let bound = method(
            SourceLanguage::Java,
            &format!("for (int i = 0; i < {}; i++) a();", terms.join(" + ")),
        );
        assert_eq!(build(SourceLanguage::Java, bound).unwrap().depth(), 2);
    });
}
