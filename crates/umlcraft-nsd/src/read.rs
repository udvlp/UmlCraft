//! Reading `.nsd` files.
//!
//! Attribute values are unescaped but not normalized: a tab Structorizer wrote stays a tab
//! (an XML parser would make it a space), so that writing gives the same bytes. Whitespace
//! between tags is skipped; the writer puts back Structorizer's indentation.

use std::borrow::Cow;

use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};

use crate::model::{Attr, Attrs, Branches, Diagram, Element, ElementKind, Sequence};

/// Why a file could not be read; positions are 1-based line and column.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("{line}:{column}: malformed XML: {message}")]
    Xml { line: usize, column: usize, message: String },
    #[error("not a Structorizer diagram: the document element is not <root>")]
    NotStructorizer,
    #[error("{line}:{column}: unexpected <{name}> in <{parent}>")]
    UnexpectedElement { line: usize, column: usize, name: String, parent: String },
    #[error("{line}:{column}: <{element}> has the branches {found:?}, expected {expected}")]
    WrongBranches {
        line: usize,
        column: usize,
        element: String,
        found: Vec<String>,
        expected: String,
    },
    #[error("{line}:{column}: unexpected text in <{parent}>")]
    UnexpectedText { line: usize, column: usize, parent: String },
    #[error("unsupported encoding {0:?}, only UTF-8")]
    Encoding(String),
    #[error("the file ends inside <{0}>")]
    Truncated(String),
    #[error("{line}:{column}: elements nested deeper than {max}")]
    TooDeep { line: usize, column: usize, max: usize },
}

/// How deep elements may nest. Structograms people draw stay far below; without a limit a
/// file nested thousands deep would overflow the stack while it is read, laid out or drawn.
pub const MAX_DEPTH: usize = 200;

/// Reads a whole `.nsd` file. A byte order mark is skipped.
pub fn read(xml: &str) -> Result<Diagram, ReadError> {
    Parser::new(xml.strip_prefix('\u{feff}').unwrap_or(xml)).diagram()
}

struct Parser<'a> {
    xml: &'a str,
    reader: Reader<&'a [u8]>,
    /// Where the last event started.
    at: usize,
    /// How many elements are open around the one being read.
    depth: usize,
}

/// What the next event means for the structure.
enum Next<'a> {
    Open(BytesStart<'a>, bool),
    /// The end tag of the open element (quick-xml checks that it matches).
    Close,
    End,
}

impl<'a> Parser<'a> {
    fn new(xml: &'a str) -> Self {
        Parser { xml, reader: Reader::from_str(xml), at: 0, depth: 0 }
    }

    fn position(&self, at: usize) -> (usize, usize) {
        let before = &self.xml[..at.min(self.xml.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        (line, column)
    }

    fn xml_error(&self, message: impl ToString) -> ReadError {
        let (line, column) = self.position(self.reader.error_position() as usize);
        ReadError::Xml { line, column, message: message.to_string() }
    }

    /// The next tag, skipping the declaration, comments and whitespace.
    fn next(&mut self, parent: &str) -> Result<Next<'a>, ReadError> {
        loop {
            self.at = self.reader.buffer_position() as usize;
            match self.reader.read_event().map_err(|e| self.xml_error(e))? {
                Event::Start(e) => return Ok(Next::Open(e, false)),
                Event::Empty(e) => return Ok(Next::Open(e, true)),
                Event::End(_) => return Ok(Next::Close),
                Event::Eof => return Ok(Next::End),
                Event::Text(t) if t.bytes().all(|b| b.is_ascii_whitespace()) => {}
                Event::Text(_) | Event::CData(_) => {
                    let (line, column) = self.position(self.at);
                    return Err(ReadError::UnexpectedText { line, column, parent: parent.into() });
                }
                Event::Decl(d) => {
                    if let Some(Ok(enc)) = d.encoding() {
                        let enc = enc.into_owned();
                        if !enc.eq_ignore_ascii_case("utf-8") {
                            return Err(ReadError::Encoding(enc));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn unexpected(&self, name: &str, parent: &str) -> ReadError {
        let (line, column) = self.position(self.at);
        ReadError::UnexpectedElement { line, column, name: name.into(), parent: parent.into() }
    }

    /// An error in the attributes of the tag that starts at `self.at` (the reader does not
    /// track a position for these).
    fn attr_error(&self, message: impl ToString) -> ReadError {
        let (line, column) = self.position(self.at);
        ReadError::Xml { line, column, message: message.to_string() }
    }

    fn attrs(&self, start: &BytesStart) -> Result<Attrs, ReadError> {
        let mut attrs = Vec::new();
        for a in start.attributes() {
            let a = a.map_err(|e| self.attr_error(e))?;
            let value = unescape(&a.value).map_err(|e| self.attr_error(e))?;
            attrs.push(Attr { name: a.key.as_ref().to_owned(), value: Cow::into_owned(value) });
        }
        Ok(Attrs(attrs))
    }

    fn diagram(mut self) -> Result<Diagram, ReadError> {
        let (root, empty) = match self.next("")? {
            Next::Open(e, empty) if e.name().as_ref() == "root" => (e, empty),
            _ => return Err(ReadError::NotStructorizer),
        };
        let attrs = self.attrs(&root)?;
        let mut children = None;
        if !empty {
            loop {
                match self.next("root")? {
                    Next::Open(e, empty)
                        if e.name().as_ref() == "children" && children.is_none() =>
                    {
                        children = Some(self.sequence(&e, empty, "children")?);
                    }
                    Next::Open(e, _) => return Err(self.unexpected(e.name().as_ref(), "root")),
                    Next::Close => break,
                    Next::End => return Err(ReadError::Truncated("root".into())),
                }
            }
        }
        Ok(Diagram { attrs, children: children.unwrap_or_default() })
    }

    /// The elements up to the end tag of `start`.
    fn sequence(
        &mut self,
        start: &BytesStart,
        empty: bool,
        tag: &str,
    ) -> Result<Sequence, ReadError> {
        let attrs = self.attrs(start)?;
        let mut elements = Vec::new();
        if !empty {
            loop {
                match self.next(tag)? {
                    Next::Open(e, empty) => {
                        let name = e.name();
                        let kind = ElementKind::from_tag(name.as_ref())
                            .ok_or_else(|| self.unexpected(name.as_ref(), tag))?;
                        elements.push(self.element(kind, &e, empty)?);
                    }
                    Next::Close => break,
                    Next::End => return Err(ReadError::Truncated(tag.into())),
                }
            }
        }
        Ok(Sequence { attrs, elements })
    }

    fn element(
        &mut self,
        kind: ElementKind,
        start: &BytesStart,
        empty: bool,
    ) -> Result<Element, ReadError> {
        let started = self.at;
        if self.depth >= MAX_DEPTH {
            let (line, column) = self.position(started);
            return Err(ReadError::TooDeep { line, column, max: MAX_DEPTH });
        }
        self.depth += 1;
        let element = self.element_inside(kind, start, empty, started);
        self.depth -= 1;
        element
    }

    fn element_inside(
        &mut self,
        kind: ElementKind,
        start: &BytesStart,
        empty: bool,
        started: usize,
    ) -> Result<Element, ReadError> {
        let attrs = self.attrs(start)?;
        let mut branches = Vec::new();
        let mut tags = Vec::new();
        if !empty {
            loop {
                match self.next(kind.tag())? {
                    Next::Open(e, empty) => {
                        let tag = e.name().as_ref().to_owned();
                        let known = match kind.branches() {
                            Branches::Fixed(fixed) => fixed.contains(&tag.as_str()),
                            Branches::Repeated(t) => t == tag,
                        };
                        if !known {
                            return Err(self.unexpected(&tag, kind.tag()));
                        }
                        branches.push(self.sequence(&e, empty, &tag)?);
                        tags.push(tag);
                    }
                    Next::Close => break,
                    Next::End => return Err(ReadError::Truncated(kind.tag().into())),
                }
            }
        }
        let (fits, expected) = match kind.branches() {
            Branches::Fixed(fixed) => (tags == fixed, format!("{fixed:?}")),
            Branches::Repeated(t) => (!tags.is_empty(), format!("one or more {t}")),
        };
        if !fits {
            let (line, column) = self.position(started);
            return Err(ReadError::WrongBranches {
                line,
                column,
                element: kind.tag().into(),
                found: tags,
                expected,
            });
        }
        Ok(Element { kind, attrs, branches })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, RootKind};

    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns:nsd="https://structorizer.fisch.lu" version="3.32-35" text="&#34;f(a: int)&#34;" comment="" color="ffffff" type="sub" style="nice">
	<children>
"#;
    const TAIL: &str = "\t</children>\n</root>";

    fn with_body(body: &str) -> String {
        format!("{HEAD}{body}{TAIL}")
    }

    #[test]
    fn reads_the_root() {
        let d = read(&with_body("")).unwrap();
        assert_eq!(d.text(), ["f(a: int)"]);
        assert_eq!(d.kind(), RootKind::Sub);
        assert_eq!(d.header("version"), Some("3.32-35"));
        assert!(d.children.elements.is_empty());
    }

    #[test]
    fn reads_every_kind_with_its_branches() {
        let body = r#"<instruction text="&#34;a &#60;- 1&#34;,&#34;b&#34;" comment="&#34;why&#34;" color="ff8080" rotated="0" disabled="1"></instruction>
<call text="&#34;g()&#34;" comment="" color="ffffff" disabled="0" ismethoddecl="0"></call>
<jump text="" comment="" color="ffffff" disabled="0"></jump>
<alternative text="&#34;a &#62; b&#34;" comment="" color="ffffff" branch_colors="80ff80," disabled="0"><qTrue color="ffffff"></qTrue><qFalse color="ffffff"></qFalse></alternative>
<case text="&#34;x&#34;,&#34;1&#34;,&#34;default&#34;" comment="" color="ffffff" branch_colors="," disabled="0"><qCase color="ffffff"></qCase><qCase color="ffffff"></qCase></case>
<for text="&#34;for i &#60;- 1 to 3&#34;" comment="" counterVar="i" startValue="1" endValue="3" stepConst="1" style="COUNTER" color="ffffff" disabled="0"><qFor color="ffffff"></qFor></for>
<while text="&#34;c&#34;" comment="" color="ffffff" disabled="0"><qWhile color="ffffff"></qWhile></while>
<repeat text="&#34;c&#34;" comment="" color="ffffff" disabled="0"><qRepeat color="ffffff"></qRepeat></repeat>
<forever comment="" color="ffffff" disabled="0"><qForever color="ffffff"></qForever></forever>
<try text="&#34;e&#34;" comment="" color="ffffff" disabled="0"><qTry color="ffffff"></qTry><qCatch color="ffffff"></qCatch><qFinally color="ffffff"></qFinally></try>
<parallel text="&#34;2&#34;" comment="" color="ffffff" disabled="0"><qPara color="ffffff"></qPara><qPara color="ffffff"></qPara></parallel>
"#;
        let d = read(&with_body(body)).unwrap();
        let kinds: Vec<_> = d.children.elements.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, ElementKind::ALL);
        let first = &d.children.elements[0];
        assert_eq!(first.text(), ["a <- 1", "b"]);
        assert_eq!(first.comment(), ["why"]);
        assert_eq!(first.color(), Color([0xff, 0x80, 0x80]));
        assert!(first.disabled());
        let alt = &d.children.elements[3];
        assert_eq!(alt.branches.len(), 2);
        assert_eq!(alt.branch_colors(), [Some(Color([0x80, 0xff, 0x80])), None]);
        assert_eq!(d.children.elements[4].branches.len(), 2);
        assert_eq!(d.children.elements[9].branches.len(), 3);
    }

    #[test]
    fn keeps_tabs_and_unknown_attributes() {
        let body =
            "<instruction text=\"&#34;a\tb&#34;\" comment=\"\" future=\"x\"></instruction>\n";
        let d = read(&with_body(body)).unwrap();
        let e = &d.children.elements[0];
        assert_eq!(e.text(), ["a\tb"]);
        assert_eq!(e.attr("future"), Some("x"));
    }

    #[test]
    fn reports_unknown_elements_with_their_place() {
        let err = read(&with_body("\t\t<loop text=\"\"></loop>\n")).unwrap_err();
        assert_eq!(
            err,
            ReadError::UnexpectedElement {
                line: 4,
                column: 3,
                name: "loop".into(),
                parent: "children".into()
            }
        );
    }

    #[test]
    fn reports_wrong_branches() {
        let body = "<alternative text=\"\" comment=\"\"><qTrue></qTrue></alternative>";
        assert!(matches!(
            read(&with_body(body)),
            Err(ReadError::WrongBranches { element, .. }) if element == "alternative"
        ));
        let body = "<case text=\"\" comment=\"\"></case>";
        assert!(matches!(read(&with_body(body)), Err(ReadError::WrongBranches { .. })));
        let body = "<while text=\"\" comment=\"\"><qFor></qFor></while>";
        assert!(matches!(read(&with_body(body)), Err(ReadError::UnexpectedElement { .. })));
    }

    #[test]
    fn attribute_errors_point_at_their_tag() {
        let xml = with_body("\t\t<instruction text=\"&bogus;\"></instruction>\n");
        match read(&xml) {
            Err(ReadError::Xml { line, .. }) => assert_eq!(line, 4),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_what_is_not_a_diagram() {
        assert_eq!(read("<html></html>"), Err(ReadError::NotStructorizer));
        assert_eq!(read(""), Err(ReadError::NotStructorizer));
        assert!(matches!(read(&with_body("text")), Err(ReadError::UnexpectedText { .. })));
        assert!(matches!(read(&HEAD[..HEAD.len() - 1]), Err(ReadError::Truncated(_))));
        assert!(matches!(read("<root a=\"1></root>"), Err(ReadError::Xml { .. })));
        let latin = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><root></root>";
        assert_eq!(read(latin), Err(ReadError::Encoding("ISO-8859-1".into())));
    }

    #[test]
    fn refuses_elements_nested_too_deep() {
        let nested = |depth: usize| {
            let open =
                r#"<while text="&#34;x&#34;" comment="" color="ffffff" disabled="0"><qWhile>"#;
            let close = "</qWhile></while>";
            with_body(&format!("{}{}", open.repeat(depth), close.repeat(depth)))
        };
        assert!(read(&nested(MAX_DEPTH)).is_ok());
        assert!(matches!(
            read(&nested(MAX_DEPTH + 1)),
            Err(ReadError::TooDeep { max: MAX_DEPTH, .. })
        ));
        // Far deeper than the stack would take: an error, not a crash.
        assert!(matches!(read(&nested(100_000)), Err(ReadError::TooDeep { .. })));
    }

    #[test]
    fn skips_a_byte_order_mark() {
        assert!(read(&format!("\u{feff}{}", with_body(""))).is_ok());
    }
}
