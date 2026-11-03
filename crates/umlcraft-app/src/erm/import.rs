//! "Import SQL dump as ER diagram …": a file, a small dialog (dialect, attributes, schemas),
//! the import in the background and a window with the result, unsaved, whose notes say what
//! was found (`docs/plan-erm-phase-5.md` § 5.6).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{ActiveTheme, Disableable, StyledExt, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::Id;
use umlcraft_erm_sql::ir::ProblemKind;
use umlcraft_erm_sql::{Attributes, Dialect, DumpError, Hint, Import, Options, Report, Schema};

/// What the window of an imported diagram says about the import.
#[derive(Clone, PartialEq, Debug)]
pub struct ImportNote {
    pub summary: String,
    /// Hints and problems; those about an element select it when clicked.
    pub lines: Vec<(Option<Id>, String)>,
}

/// Asks for a dump and imports it; problems are shown over `window`.
pub fn prompt(window: &mut Window, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else { return };
        let Some(path) = paths.into_iter().next() else { return };
        let job = path.clone();
        let read = cx
            .background_executor()
            .spawn(async move {
                let bytes = std::fs::read(&job).map_err(|e| e.to_string())?;
                let schema = umlcraft_erm_sql::read(&bytes, None).map_err(|e| dump_error(&e))?;
                Ok::<_, String>((Arc::new(bytes), schema))
            })
            .await;
        handle
            .update(cx, |_, window, cx| match read {
                Ok((bytes, schema)) => choose(path, bytes, schema, window, cx),
                Err(detail) => failed(&detail, window, cx),
            })
            .ok();
    })
    .detach();
}

/// The dialect, the attributes and the schemas to take.
struct Choice {
    dialect: Dialect,
    attributes: Attributes,
    schemas: BTreeSet<usize>,
}

const ATTRIBUTES: [Attributes; 3] = [Attributes::All, Attributes::Keys, Attributes::None];

fn choose(path: PathBuf, bytes: Arc<Vec<u8>>, schema: Schema, window: &mut Window, cx: &mut App) {
    if schema.tables.is_empty() {
        failed(&t!("erm.import_no_tables"), window, cx);
        return;
    }
    let detected = schema.dialect;
    let schemas = schema.schemas();
    let choice = Rc::new(RefCell::new(Choice {
        dialect: detected,
        attributes: Attributes::All,
        schemas: (0..schemas.len()).collect(),
    }));
    let file = file_name(&path);
    let tables = schema.tables.len();
    let job = Rc::new((path, bytes, schema));
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        let current = choice.borrow();
        let c1 = choice.clone();
        let dialects = RadioGroup::horizontal("erm-import-dialect")
            .children(Dialect::ALL.map(|d| {
                if d == detected {
                    t!("erm.import_detected", dialect = d.name()).to_string()
                } else {
                    d.name().to_owned()
                }
            }))
            .selected_index(Dialect::ALL.iter().position(|&d| d == current.dialect))
            .on_change(move |&ix, window, _| {
                c1.borrow_mut().dialect = Dialect::ALL[ix];
                window.refresh();
            });
        let c2 = choice.clone();
        let attributes = RadioGroup::horizontal("erm-import-attributes")
            .children([
                t!("erm.import_attributes_all").to_string(),
                t!("erm.import_attributes_keys").to_string(),
                t!("erm.import_attributes_none").to_string(),
            ])
            .selected_index(ATTRIBUTES.iter().position(|&a| a == current.attributes))
            .on_change(move |&ix, window, _| {
                c2.borrow_mut().attributes = ATTRIBUTES[ix];
                window.refresh();
            });
        let mut body =
            v_flex()
                .gap_4()
                .pb_1()
                .child(div().text_sm().child(
                    t!("erm.import_found", file = file.as_str(), count = tables).to_string(),
                ))
                .child(section(t!("erm.import_dialect").to_string(), dialects))
                .child(section(
                    t!("erm.import_attributes").to_string(),
                    v_flex().gap_1().child(attributes).child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(t!("erm.import_attributes_hint").to_string()),
                    ),
                ));
        if schemas.len() > 1 {
            let mut list = h_flex().gap_4().flex_wrap();
            for (i, name) in schemas.iter().enumerate() {
                let toggle = choice.clone();
                list = list.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Checkbox::new(("erm-import-schema", i))
                                .checked(current.schemas.contains(&i))
                                .on_click(move |&on, window, _| {
                                    let mut c = toggle.borrow_mut();
                                    if on {
                                        c.schemas.insert(i);
                                    } else {
                                        c.schemas.remove(&i);
                                    }
                                    window.refresh();
                                }),
                        )
                        .child(div().text_sm().child(name.clone())),
                );
            }
            body = body.child(section(t!("erm.import_schemas").to_string(), list));
        }
        let none = current.schemas.is_empty();
        drop(current);
        let go = {
            let (choice, job, schemas) = (choice.clone(), job.clone(), schemas.clone());
            move |window: &mut Window, cx: &mut App| {
                let c = choice.borrow();
                if c.schemas.is_empty() {
                    return;
                }
                window.close_dialog(cx);
                let picked: Vec<String> = c.schemas.iter().map(|&i| schemas[i].clone()).collect();
                let options = Options {
                    attributes: c.attributes,
                    schemas: (picked.len() < schemas.len()).then_some(picked),
                    file_name: Some(file_name(&job.0)),
                };
                run(job.clone(), c.dialect, options, window, cx);
            }
        };
        let go_click = go.clone();
        dialog
            .title(t!("erm.import_title").to_string())
            .width(px(520.0))
            .child(body)
            .on_ok(move |_, window, cx| {
                go(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("erm-import-cancel")
                            .label(t!("dialog.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("erm-import-go")
                            .primary()
                            .label(t!("erm.import_button").to_string())
                            .disabled(none)
                            .on_click(move |_, window, cx| go_click(window, cx)),
                    ),
            )
    });
}

/// Maps and lays out in the background (reading the dump again if another dialect was
/// chosen), then opens the window.
fn run(
    job: Rc<(PathBuf, Arc<Vec<u8>>, Schema)>,
    dialect: Dialect,
    options: Options,
    window: &mut Window,
    cx: &mut App,
) {
    let (path, bytes, schema) = (job.0.clone(), job.1.clone(), job.2.clone());
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                let mut options = options;
                let schema = if schema.dialect == dialect {
                    schema
                } else {
                    // Schemas are a matter of the dialect: those offered were PostgreSQL's.
                    options.schemas = None;
                    umlcraft_erm_sql::read(&bytes, Some(dialect)).map_err(|e| dump_error(&e))?
                };
                Ok::<Import, String>(umlcraft_erm_sql::import(&schema, &options))
            })
            .await;
        handle
            .update(cx, |_, window, cx| match result {
                Ok(import) if import.report.tables == 0 => {
                    failed(&t!("erm.import_no_tables"), window, cx)
                }
                Ok(import) => {
                    let note = note(&import, &file_name(&path));
                    super::open_imported(import.diagram, note, &path, cx);
                }
                Err(detail) => failed(&detail, window, cx),
            })
            .ok();
    })
    .detach();
}

/// The summary and the lines under it.
pub fn note(import: &Import, file: &str) -> ImportNote {
    let r: &Report = &import.report;
    let summary = t!(
        "erm.import_summary",
        file = file,
        dialect = r.dialect.name(),
        tables = r.tables,
        entities = r.entities,
        relationships = r.relationships,
        attributes = r.attributes,
        skipped = r.skipped_total()
    )
    .to_string();
    let mut lines: Vec<(Option<Id>, String)> = r
        .hints
        .iter()
        .map(|h| {
            let text = match h {
                Hint::NoPrimaryKey { table, .. } => t!("erm.import_no_key", table = table),
                Hint::NotInDump { table, .. } => t!("erm.import_placeholder", table = table),
                Hint::Recoded { line } => t!("erm.import_recoded", line = line),
            };
            (h.element().cloned(), text.to_string())
        })
        .collect();
    lines.extend(r.problems.iter().map(|p| {
        let (line, text) = (p.line, p.text.as_str());
        let what = match p.kind {
            ProblemKind::Statement => t!("erm.import_problem_statement"),
            ProblemKind::Column => t!("erm.import_problem_column"),
            ProblemKind::Constraint => t!("erm.import_problem_constraint"),
            ProblemKind::UnknownTable => {
                return (
                    None,
                    t!("erm.import_unknown_table", line = line, text = text).to_string(),
                );
            }
        };
        (None, t!("erm.import_problem", line = line, what = what, text = text).to_string())
    }));
    ImportNote { summary, lines }
}

fn dump_error(e: &DumpError) -> String {
    match e {
        DumpError::PgCustomFormat => t!("erm.import_pg_custom").to_string(),
        DumpError::Compressed => t!("erm.import_compressed").to_string(),
    }
}

fn failed(detail: &str, window: &mut Window, cx: &mut App) {
    let rx = window.prompt(
        PromptLevel::Critical,
        &t!("erm.import_failed"),
        Some(detail),
        &[t!("dialog.ok").as_ref()],
        cx,
    );
    // Dropping the answer would cancel the dialog before it appears.
    cx.spawn(async move |_| rx.await.ok()).detach();
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A heading with what it sets below.
fn section(title: String, body: impl IntoElement) -> impl IntoElement {
    v_flex().gap_2().child(div().text_sm().font_semibold().child(title)).child(body)
}

#[cfg(test)]
mod tests {
    use umlcraft_erm_sql::{Dialect, Options};

    use super::note;

    #[test]
    fn the_note_counts_and_lists_hints_and_problems() {
        let sql = b"CREATE TABLE a (id int, b_id int REFERENCES b (id));\nCREATE TABLE c (x int, 'y' int);";
        let schema = umlcraft_erm_sql::read(sql, Some(Dialect::MySql)).unwrap();
        let import = umlcraft_erm_sql::import(&schema, &Options::default());
        let n = note(&import, "shop.sql");
        assert!(n.summary.contains("shop.sql"), "{}", n.summary);
        // a and c have no key, b is a placeholder, c has a column that cannot be read.
        assert_eq!(n.lines.len(), 4, "{:?}", n.lines);
        assert!(n.lines[..3].iter().all(|(id, _)| id.is_some()));
        assert!(n.lines[3].0.is_none() && n.lines[3].1.contains('2'));
    }
}
