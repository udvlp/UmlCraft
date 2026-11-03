//! "Import SQL dump as ER diagram …": a file, a small dialog (dialect, notation, attributes,
//! schemas),
//! and the import into the project: a package named after the database with an ER diagram,
//! which opens. What was found goes to the status bar; hints and problems to a message.

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
use umlcraft_erm_sql::ir::ProblemKind;
use umlcraft_erm_sql::{Attributes, Dialect, DumpError, Hint, Options, Report, Schema};
use umlcraft_model::ErNotation;

use crate::main_view::MainView;

/// Asks for a dump, reads it in the background and offers the dialog; problems are shown over
/// `window`.
pub fn prompt(view: WeakEntity<MainView>, window: &mut Window, cx: &mut App) {
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
                Ok((bytes, schema)) => choose(view, path, bytes, schema, window, cx),
                Err(detail) => failed(&detail, window, cx),
            })
            .ok();
    })
    .detach();
}

/// The dialect, the notation, the attributes and the schemas to take.
struct Choice {
    dialect: Dialect,
    notation: ErNotation,
    attributes: Attributes,
    schemas: BTreeSet<usize>,
}

const ATTRIBUTES: [Attributes; 3] = [Attributes::All, Attributes::Keys, Attributes::None];
const NOTATIONS: [ErNotation; 3] = [ErNotation::Chen, ErNotation::Ie, ErNotation::Idef1x];

fn choose(
    view: WeakEntity<MainView>,
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    schema: Schema,
    window: &mut Window,
    cx: &mut App,
) {
    if schema.tables.is_empty() {
        failed(&t!("sql.no_tables"), window, cx);
        return;
    }
    let detected = schema.dialect;
    let schemas = schema.schemas();
    let choice = Rc::new(RefCell::new(Choice {
        dialect: detected,
        notation: ErNotation::Chen,
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
        let dialects = RadioGroup::horizontal("sql-import-dialect")
            .children(Dialect::ALL.map(|d| {
                if d == detected {
                    t!("sql.detected", dialect = d.name()).to_string()
                } else {
                    d.name().to_owned()
                }
            }))
            .selected_index(Dialect::ALL.iter().position(|&d| d == current.dialect))
            .on_change(move |&ix, window, _| {
                c1.borrow_mut().dialect = Dialect::ALL[ix];
                window.refresh();
            });
        let c3 = choice.clone();
        let notations = RadioGroup::horizontal("sql-import-notation")
            .children(
                ["chen", "ie", "idef1x"].map(|k| t!(format!("ribbon.notation.{k}")).to_string()),
            )
            .selected_index(NOTATIONS.iter().position(|&n| n == current.notation))
            .on_change(move |&ix, window, _| {
                c3.borrow_mut().notation = NOTATIONS[ix];
                window.refresh();
            });
        let c2 = choice.clone();
        let attributes = RadioGroup::horizontal("sql-import-attributes")
            .children([
                t!("sql.attributes_all").to_string(),
                t!("sql.attributes_keys").to_string(),
                t!("sql.attributes_none").to_string(),
            ])
            .selected_index(ATTRIBUTES.iter().position(|&a| a == current.attributes))
            .on_change(move |&ix, window, _| {
                c2.borrow_mut().attributes = ATTRIBUTES[ix];
                window.refresh();
            });
        let mut body = v_flex()
            .gap_4()
            .pb_1()
            .child(
                div()
                    .text_sm()
                    .child(t!("sql.found", file = file.as_str(), count = tables).to_string()),
            )
            .child(section(t!("sql.dialect").to_string(), dialects))
            .child(section(t!("sql.notation").to_string(), notations))
            .child(section(
                t!("sql.attributes").to_string(),
                v_flex()
                    .gap_1()
                    .child(attributes)
                    .child(div().text_xs().text_color(muted).child(t!("sql.attributes_hint"))),
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
                            Checkbox::new(("sql-import-schema", i))
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
            body = body.child(section(t!("sql.schemas").to_string(), list));
        }
        let none = current.schemas.is_empty();
        drop(current);
        let go = {
            let (choice, job, schemas, view) =
                (choice.clone(), job.clone(), schemas.clone(), view.clone());
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
                    notation: c.notation,
                };
                run(view.clone(), job.clone(), c.dialect, options, window, cx);
            }
        };
        let go_click = go.clone();
        dialog
            .title(t!("sql.title").to_string())
            .width(px(520.0))
            .child(body)
            .on_ok(move |_, window, cx| {
                go(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("sql-import-cancel")
                            .label(t!("dialog.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("sql-import-go")
                            .primary()
                            .label(t!("sql.button").to_string())
                            .disabled(none)
                            .on_click(move |_, window, cx| go_click(window, cx)),
                    ),
            )
    });
}

/// Reads the dump again if another dialect was chosen, then imports it into the project.
fn run(
    view: WeakEntity<MainView>,
    job: Rc<(PathBuf, Arc<Vec<u8>>, Schema)>,
    dialect: Dialect,
    options: Options,
    window: &mut Window,
    cx: &mut App,
) {
    let (path, bytes, schema) = (job.0.clone(), job.1.clone(), job.2.clone());
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move {
                let mut options = options;
                if schema.dialect == dialect {
                    return Ok((schema, options));
                }
                // Schemas are a matter of the dialect: those offered were PostgreSQL's.
                options.schemas = None;
                let schema =
                    umlcraft_erm_sql::read(&bytes, Some(dialect)).map_err(|e| dump_error(&e))?;
                Ok::<_, String>((schema, options))
            })
            .await;
        handle
            .update(cx, |_, window, cx| match read {
                Ok((schema, options)) if !has_tables(&schema, &options) => {
                    failed(&t!("sql.no_tables"), window, cx);
                }
                Ok((schema, options)) => {
                    let file = file_name(&path);
                    view.update(cx, |view, cx| {
                        view.import_sql(&schema, &options, &file, window, cx)
                    })
                    .ok();
                }
                Err(detail) => failed(&detail, window, cx),
            })
            .ok();
    })
    .detach();
}

/// Whether the schemas chosen have tables.
fn has_tables(schema: &Schema, options: &Options) -> bool {
    let taken = |key: &umlcraft_erm_sql::ir::TableKey| {
        options.schemas.as_ref().is_none_or(|s| s.contains(&key.schema))
    };
    schema.tables.keys().any(taken)
}

/// The summary, and the hints and problems one per line.
pub fn report_text(report: &Report, file: &str) -> (String, Vec<String>) {
    let r = report;
    let summary = t!(
        "sql.summary",
        file = file,
        dialect = r.dialect.name(),
        tables = r.tables,
        entities = r.entities,
        relationships = r.relationships,
        attributes = r.attributes,
        skipped = r.skipped_total()
    )
    .to_string();
    let mut lines: Vec<String> = r
        .hints
        .iter()
        .map(|h| match h {
            Hint::NoPrimaryKey { table, .. } => t!("sql.no_key", table = table),
            Hint::NotInDump { table, .. } => t!("sql.placeholder", table = table),
            Hint::Recoded { line } => t!("sql.recoded", line = line),
        })
        .map(|text| text.to_string())
        .collect();
    lines.extend(r.problems.iter().map(|p| {
        let (line, text) = (p.line, p.text.as_str());
        let what = match p.kind {
            ProblemKind::Statement => t!("sql.problem_statement"),
            ProblemKind::Column => t!("sql.problem_column"),
            ProblemKind::Constraint => t!("sql.problem_constraint"),
            ProblemKind::UnknownTable => {
                return t!("sql.unknown_table", line = line, text = text).to_string();
            }
        };
        t!("sql.problem", line = line, what = what, text = text).to_string()
    }));
    (summary, lines)
}

fn dump_error(e: &DumpError) -> String {
    match e {
        DumpError::PgCustomFormat => t!("sql.pg_custom").to_string(),
        DumpError::Compressed => t!("sql.compressed").to_string(),
    }
}

fn failed(detail: &str, window: &mut Window, cx: &mut App) {
    let rx = window.prompt(
        PromptLevel::Critical,
        &t!("sql.failed"),
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
    use umlcraft_model::{Document, Model};

    use super::report_text;

    #[test]
    fn the_report_counts_and_lists_hints_and_problems() {
        let sql = b"CREATE TABLE a (id int, b_id int REFERENCES b (id));\nCREATE TABLE c (x int, 'y' int);";
        let schema = umlcraft_erm_sql::read(sql, Some(Dialect::MySql)).unwrap();
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let mut report = None;
        doc.edit("import", |tx| {
            report =
                Some(umlcraft_erm_sql::map::map(tx, root, &schema, &Options::default())?.report);
            Ok(())
        })
        .unwrap();
        let (summary, lines) = report_text(&report.unwrap(), "shop.sql");
        assert!(summary.contains("shop.sql"), "{summary}");
        // a and c have no key, b is a placeholder, c has a column that cannot be read.
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[3].contains('2'), "{lines:?}");
    }
}
