//! Single statements and what the report says of them, where the fixtures do not show it.

mod common;

use common::MADE;
use umlcraft_nsd::{Diagram, ElementKind};
use umlcraft_nsd_code::{NoteKind, Report, Source, SourceLanguage, Unbuilt};

/// The diagram of the first method in `text`.
fn first(language: SourceLanguage, text: &str) -> (Diagram, Report) {
    let source = Source::parse(language, text);
    source.diagram(&source.methods()[0], &MADE).unwrap()
}

fn texts(d: &Diagram) -> Vec<Vec<String>> {
    d.children.elements.iter().map(|e| e.text()).collect()
}

fn kinds(report: &Report) -> Vec<(usize, NoteKind)> {
    report.notes.iter().map(|n| (n.line, n.kind)).collect()
}

#[test]
fn a_class_initialized_with_one_argument_is_constructed() {
    let text = "void f(int n) {\n  std::vector<int> v(10);\n  std::vector<int> w{n + 1};\n  int i(3);\n  auto a{4};\n}\n";
    let (d, _) = first(SourceLanguage::Cpp, text);
    assert_eq!(
        texts(&d),
        [
            vec!["v := std::vector<int>(10)"],
            vec!["w := std::vector<int>{n + 1}"],
            vec!["i := 3"],
            vec!["a := 4"],
        ]
    );
}

#[test]
fn a_python_statement_of_several_expressions_keeps_each() {
    let (d, _) = first(SourceLanguage::Python, "def f():\n    g(), h()\n\ndef g():\n    pass\n");
    assert_eq!(texts(&d), [vec!["g()"], vec!["h()"]]);
    assert_eq!(d.children.elements[0].kind, ElementKind::Call);
    assert_eq!(d.children.elements[1].kind, ElementKind::Instruction);
}

#[test]
fn a_conversion_operator_is_named_by_its_keyword() {
    let text = "class A {\n  public static explicit operator B(A a) { var implicitly = 1; return null; }\n}\n";
    let methods = Source::parse(SourceLanguage::CSharp, text).methods();
    assert_eq!(methods[0].name, "op_Explicit");
}

/// The file changed between listing and building: another method at the same place is not
/// built under the old one's name.
#[test]
fn a_java_method_is_found_again_by_its_name_too() {
    let before = Source::parse(SourceLanguage::Java, "class A { void f() { } }");
    let method = before.methods().remove(0);
    let after = Source::parse(SourceLanguage::Java, "class A { void g() { } }");
    assert_eq!(after.diagram(&method, &MADE), Err(Unbuilt::NotFound));
}

#[test]
fn a_break_inside_a_case_is_named_in_the_report() {
    let java = "class A {\n  void f(int x) {\n    switch (x) {\n      case 1:\n        if (x > 0) break;\n        g();\n        break;\n      case 2: {\n        while (true) { break; }\n        break;\n      }\n    }\n  }\n}\n";
    let (_, report) = first(SourceLanguage::Java, java);
    assert_eq!(kinds(&report), [(5, NoteKind::CaseBreak)]);
    let cpp =
        "void f(int x) {\n  switch (x) {\n    case 1:\n      if (x) break;\n      break;\n  }\n}\n";
    let (_, report) = first(SourceLanguage::Cpp, cpp);
    assert_eq!(kinds(&report), [(4, NoteKind::CaseBreak)]);
}

#[test]
fn a_bound_at_the_end_of_the_numbers_is_not_computed() {
    let text = "class A {\n  void f() {\n    for (long i = 0; i < -9223372036854775808; i++) { }\n  }\n}\n";
    let (d, _) = first(SourceLanguage::Java, text);
    assert_eq!(texts(&d)[0], ["for i := 0 to -9223372036854775808 - 1"]);
    let text = "def f():\n    for i in range(9223372036854775807 + 1):\n        pass\n";
    let (d, _) = first(SourceLanguage::Python, text);
    assert_eq!(texts(&d)[0], ["for i := 0 to 9223372036854775807"]);
}

#[test]
fn code_switched_off_with_if_0_is_left_out_and_named() {
    let text = "class A {\n  void f() {\n#if false\n    Old(;\n#else\n    New();\n#endif\n  }\n}\n";
    let (d, report) = first(SourceLanguage::CSharp, text);
    assert_eq!(texts(&d), [vec!["New()"]]);
    assert_eq!(kinds(&report), [(3, NoteKind::Conditional)]);
    let text = "void f() {\n#if 0\n  old(;\n#endif\n  now();\n}\n";
    let (d, report) = first(SourceLanguage::Cpp, text);
    assert_eq!(texts(&d), [vec!["now()"]]);
    assert_eq!(kinds(&report), [(2, NoteKind::Conditional)]);
}

/// The diagrams of a file are built in one pass over its members: thousands of methods take
/// no longer than reading them.
#[test]
fn every_method_of_a_large_file_is_built_quickly() {
    let n = 3_000;
    let csharp: String =
        (0..n).map(|i| format!("  int M{i}(int x) {{ return x + {i}; }}\n")).collect();
    let python: String =
        (0..n).map(|i| format!("def f{i}(x):\n    return f{}(x)\n", (i + 1) % n)).collect();
    for (language, text) in [
        (SourceLanguage::CSharp, format!("class A {{\n{csharp}}}\n")),
        (SourceLanguage::Python, python),
    ] {
        let start = std::time::Instant::now();
        let source = Source::parse(language, text);
        let methods = source.methods();
        assert_eq!(methods.len(), n);
        let built = source.diagrams(&methods, &MADE);
        assert!(built.iter().all(Result::is_ok));
        assert!(start.elapsed().as_secs() < 10, "{language:?}: {:?}", start.elapsed());
    }
}

#[test]
fn the_break_that_ends_a_case_goes_with_a_comment_after_it() {
    let java = "class A {\n  void f(int x) {\n    switch (x) {\n      case 1: {\n        g();\n        break; // done\n      }\n      default:\n        h();\n    }\n  }\n}\n";
    let (d, report) = first(SourceLanguage::Java, java);
    let case = &d.children.elements[0];
    assert_eq!(case.kind, ElementKind::Case);
    let jumps = case.branches[0].elements.iter().filter(|e| e.kind == ElementKind::Jump).count();
    assert_eq!(jumps, 0, "{:?}", case.branches[0].elements);
    assert_eq!(kinds(&report), []);
}
