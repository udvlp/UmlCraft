//! Exporting images and SQL.

use super::*;

pub(super) fn export_image(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let path = access.check_write(Path::new(str_arg(args, "path")?), None)?;
    let bytes = match (shown(host, args)?, extension(&path).as_str()) {
        (Shown::Structogram(d), "nsd") => {
            let origin = format!("UmlCraft {}", env!("CARGO_PKG_VERSION"));
            let made = umlcraft_nsd::Provenance { author: "", date: "", origin: &origin };
            umlcraft_nsd::write(&umlcraft_nsd::content::to_file(d, &made)).into_bytes()
        }
        (Shown::Structogram(d), extension) => {
            let svg = structogram_svg(d);
            match extension {
                "svg" => svg.into_bytes(),
                // Two pixels per pixel of the structogram, as diagrams get two per unit.
                "png" => png(&svg, 2.0 * svg::SCALE)?,
                _ => return Err("a structogram goes to .svg, .png or .nsd".into()),
            }
        }
        (Shown::Diagram(diagram), extension) => {
            let svg = diagram_svg(host, diagram);
            match extension {
                "svg" => svg.into_bytes(),
                "png" => png(&svg, 2.0)?,
                _ => return Err("the image file needs the extension .svg or .png".into()),
            }
        }
    };
    write_export(&path, &bytes, args)?;
    Ok(Reply::json(json!({ "written": path })))
}

/// Writes an export to `path`, creating its folder; a file that is there already only with
/// `overwrite`, so that a call cannot replace a document by mistake. The bytes go to a file
/// beside it that then takes its place, so that a hard link to a file elsewhere is replaced
/// rather than written through.
pub(super) fn write_export(path: &Path, bytes: &[u8], args: &Value) -> Result<(), String> {
    let overwrite = bool_arg(args, "overwrite")?.unwrap_or(false);
    if !overwrite && path.exists() {
        return Err(format!(
            "{} exists; give `\"overwrite\": true` to replace it, or another path",
            path.display()
        ));
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    crate::host::write_replacing(path, bytes)
}

pub(super) const DIALECTS: [(&str, Dialect); 3] =
    [("mysql", Dialect::MySql), ("mariadb", Dialect::MariaDb), ("postgres", Dialect::Postgres)];

pub(super) fn export_sql(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if kind != DiagramKind::EntityRelationship {
        return Err(format!("{diagram} is not an ER diagram"));
    }
    let model = host.document().model();
    let dialect = pick(args, "dialect", &DIALECTS)?
        .or_else(|| umlcraft_erm_sql::imported_dialect(model, diagram))
        .unwrap_or(Dialect::MySql);
    let out = umlcraft_erm_sql::ddl(model, diagram, dialect).map_err(|e| e.to_string())?;
    let left_out: Vec<String> = out
        .left_out
        .iter()
        .map(|l| match l {
            LeftOut::Multivalued { table, attribute } => {
                format!("{table}.{attribute}: multivalued, no column")
            }
            LeftOut::NoKey { table, target } => {
                format!("{table} → {target}: {target} has no primary key, no foreign key")
            }
            LeftOut::Check { table, check } => {
                format!("{table}: CHECK ({check}) cannot be written in this dialect")
            }
            LeftOut::Generated { table, column } => {
                format!("{table}.{column}: generated in the other dialect, a plain column here")
            }
            LeftOut::AutoIncrement { table, column } => format!(
                "{table}.{column}: MySQL only counts a column that leads the key or is unique, \
                 no AUTO_INCREMENT"
            ),
            LeftOut::Default { table, column, default } => {
                format!(
                    "{table}.{column}: `{default}` has no counterpart in this dialect, left out"
                )
            }
        })
        .collect();
    let mut reply = json!({
        "dialect": dialect.name(),
        "tables": out.tables,
        "foreign_keys": out.foreign_keys,
        "left_out": left_out,
    });
    match opt_str(args, "path") {
        Some(path) => {
            let mut path = PathBuf::from(path);
            if extension(&path) != "sql" {
                path.as_mut_os_string().push(".sql");
            }
            let path = access.check_write(&path, None)?;
            write_export(&path, out.sql.as_bytes(), args)?;
            reply["written"] = json!(path);
        }
        None => reply["sql"] = json!(out.sql),
    }
    Ok(Reply::json(reply))
}
