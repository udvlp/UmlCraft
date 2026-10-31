//! Each file in `tests/fixtures/invalid` breaks one rule and is rejected with that rule's
//! error. Files named `pN-…` break rule PN of `check`; `format-…` files break the format.

use std::path::Path;

use umlcraft_erm::{IssueKind, ReadError, read};

fn read_invalid(name: &str) -> ReadError {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/invalid").join(name);
    let file = std::fs::read_to_string(&path).unwrap();
    match read(&file) {
        Ok(_) => panic!("{name} was read without error"),
        Err(e) => e,
    }
}

/// A fixture name and how to recognize the issue it must produce.
type Rule = (&'static str, fn(&IssueKind) -> bool);

#[test]
fn every_rule_has_a_rejected_file() {
    let rules: &[Rule] = &[
        ("p1-duplicate-id", |k| *k == IssueKind::DuplicateId),
        ("p2-missing-end", |k| matches!(k, IssueKind::MissingEnd { end } if end.0 == "nowhere")),
        ("p3-not-allowed", |k| *k == IssueKind::NotAllowed { source: "entity", target: "entity" }),
        ("p4-no-owner", |k| *k == IssueKind::NoOwner),
        ("p4-several-owners", |k| *k == IssueKind::SeveralOwners),
        ("p5-own-part", |k| *k == IssueKind::OwnPart),
        ("p6-too-few-participants", |k| *k == IssueKind::TooFewParticipants),
        ("p7-identifying-without-weak", |k| *k == IssueKind::IdentifyingWithoutWeak),
        ("p9-is-a-ends", |k| *k == IssueKind::IsAEnds),
        ("p9-super-type-outside-is-a", |k| *k == IssueKind::SuperTypeOutsideIsA),
        ("p11-bad-geometry", |k| *k == IssueKind::BadGeometry),
        ("p12-bad-cardinality", |k| *k == IssueKind::BadCardinality),
    ];
    for (name, expected) in rules {
        match read_invalid(&format!("{name}.ucerm")) {
            ReadError::Invalid(issues) => {
                assert!(issues.iter().any(|i| expected(&i.kind)), "{name}: {issues:?}");
                let code = name.split('-').next().unwrap().to_uppercase();
                assert!(issues.iter().any(|i| i.kind.code() == code), "{name}: {issues:?}");
            }
            other => panic!("{name}: {other}"),
        }
    }
}

#[test]
fn format_errors_say_what_and_where() {
    let message = |name: &str| read_invalid(&format!("{name}.ucerm")).to_string();
    let unknown = message("format-unknown-field");
    assert!(unknown.contains("unknown field `colour`") && unknown.contains("line 18"), "{unknown}");
    assert_eq!(
        message("format-field-of-other-kind"),
        "x: field `weak` does not belong to type `attribute`"
    );
    assert_eq!(message("format-bad-max"), "l1: `max` is a number or \"n\", not `many`");
    assert_eq!(
        message("format-label-text-and-min"),
        "l1: a label has either `text` or `min` and `max`"
    );
    assert!(matches!(
        read_invalid("format-too-new.ucerm"),
        ReadError::TooNew { found: 2, supported: 1 }
    ));
    assert!(matches!(read_invalid("format-not-erm.ucerm"), ReadError::NotAnErmFile));
}

#[test]
fn broken_json_names_line_and_column() {
    let e =
        read("{\n  \"format\": \"umlcraft-erm\",\n  \"formatVersion\": 1,\n  oops\n}").unwrap_err();
    assert!(e.to_string().contains("line 4 column 3"), "{e}");
}
