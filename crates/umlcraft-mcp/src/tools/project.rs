//! Showing diagrams, the selection, and opening and saving projects.

use super::*;

pub(super) fn open_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let diagram = id_arg(args, "diagram")?;
    let model = host.document().model();
    let structogram = model.get(diagram).is_some_and(|e| e.kind().as_structogram().is_some());
    if !structogram {
        diagram_of(model, diagram)?;
    }
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "shown_diagram": diagram })))
}

pub(super) fn select(host: &mut dyn Host, args: &Value) -> Outcome {
    let target = match opt_id(args, "diagram")? {
        Some(id) => Some(id),
        None => host.active_diagram(),
    };
    if let Some(id) = target
        && structogram_of(host.document().model(), id).is_ok()
    {
        return select_in_structogram(host, id, args);
    }
    if opt_str(args, "path").is_some() {
        return Err("`path` selects in a structogram; give one as `diagram`, or show one".into());
    }
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let ids = ids_arg(args, "ids")?.ok_or("`ids` is missing")?;
    let model = host.document().model();
    let presentations =
        ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect::<Result<Vec<_>, _>>()?;
    host.show_diagram(diagram);
    host.select(presentations.clone());
    Ok(Reply::json(json!({ "diagram": diagram, "selection": presentations })))
}

/// `select` in a structogram: the element at `path`, the structogram itself (its id in `ids`)
/// or nothing (no `path`, no `ids`).
pub(super) fn select_in_structogram(host: &mut dyn Host, id: Id, args: &Value) -> Outcome {
    let path = opt_str(args, "path");
    let ids = ids_arg(args, "ids")?.unwrap_or_default();
    let d = &structogram_of(host.document().model(), id)?.structogram;
    let selection = match (path, ids.as_slice()) {
        (Some(path), []) => umlcraft_nsd::Selection::element(&structogram::locate(d, path)?),
        (None, []) => umlcraft_nsd::Selection::Nothing,
        (None, [only]) if *only == id => umlcraft_nsd::Selection::Root,
        _ => {
            return Err("In a structogram `path` names the element to select (`body.1.then.0`), \
                        the structogram's own id in `ids` selects it as a whole, and no `path` \
                        and an empty `ids` clear the selection."
                .into());
        }
    };
    let whole = selection == umlcraft_nsd::Selection::Root;
    host.show_diagram(id);
    host.select_in_structogram(id, selection);
    Ok(Reply::json(json!({ "structogram": id, "path": path, "whole": whole })))
}

pub(super) fn new_project(host: &mut dyn Host, args: &Value) -> Outcome {
    if args.get("sample").and_then(Value::as_bool) == Some(true) {
        let doc = host.sample()?;
        let diagram = first_diagram(doc.model());
        host.replace(doc, None)?;
        return Ok(Reply::json(json!({ "diagram": diagram })));
    }
    let doc = new_document(opt_str(args, "name").unwrap_or("Project"));
    let diagram = first_diagram(doc.model());
    host.replace(doc, None)?;
    Ok(Reply::json(json!({ "diagram": diagram })))
}

pub(super) fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub(super) fn open_file(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let path = access.check(Path::new(str_arg(args, "path")?), None)?;
    if extension(&path) != "ucraft" {
        return Err("only UmlCraft projects (.ucraft) can be opened".into());
    }
    let doc = crate::host::open(&path)?;
    let project = doc.model().get(doc.model().root()).map(|e| e.name().to_owned());
    host.replace(doc, Some(path.clone()))?;
    Ok(Reply::json(json!({ "opened": path, "project": project })))
}

pub(super) fn save_file(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let own = host.path().map(Path::to_path_buf);
    let path: PathBuf = match (opt_str(args, "path"), &own) {
        (Some(path), _) => {
            let mut path = PathBuf::from(path);
            if extension(&path) != "ucraft" {
                path.as_mut_os_string().push(".ucraft");
            }
            access.check(&path, own.as_deref())?
        }
        (None, Some(own)) => own.clone(),
        (None, None) => return Err("the project has no file yet; give `path`".into()),
    };
    umlcraft_format::save(host.document().model(), &path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (doc, _) = host.parts();
    doc.mark_saved();
    host.saved(path.clone());
    Ok(Reply::json(json!({ "saved": path })))
}
