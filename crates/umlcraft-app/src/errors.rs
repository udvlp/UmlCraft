//! Error messages in the language of the UI, naming elements instead of showing their ids.

use rust_i18n::t;
use umlcraft_format::FormatError;
use umlcraft_model::{Id, Model, ModelError, ParseCardinalityError, ParseMultiplicityError};
use umlcraft_notation::composite;

use crate::labels::display_name;

/// An element by its name; `model` is the one the error came from, if it is still at hand.
fn named(model: Option<&Model>, id: Id) -> String {
    match model.and_then(|m| m.get(id)) {
        Some(e) => t!("error.named", name = display_name(e)).to_string(),
        None => t!("error.unnamed").to_string(),
    }
}

fn kind(type_name: &str) -> String {
    t!(format!("kind.{type_name}")).to_string()
}

/// Why the model refused an edit, or why a file holds no valid model.
pub fn model_error(model: Option<&Model>, err: &ModelError) -> String {
    let a = |id: Id| named(model, id);
    match err {
        ModelError::NotFound(id) => t!("error.not_found", a = a(*id)),
        ModelError::DuplicateId(_) => t!("error.duplicate_id"),
        ModelError::InvalidOwner { owner, owner_type, child, child_type } => t!(
            "error.invalid_owner",
            child = a(*child),
            child_kind = kind(child_type),
            owner = a(*owner),
            owner_kind = kind(owner_type)
        ),
        ModelError::IndexOutOfRange { .. } => t!("error.index_out_of_range"),
        ModelError::Root => t!("error.root"),
        ModelError::HasChildren(id) => t!("error.has_children", a = a(*id)),
        ModelError::StillReferenced { target, referrer } => {
            t!("error.still_referenced", a = a(*target), b = a(*referrer))
        }
        ModelError::StillShown { target, diagram } => {
            t!("error.still_shown", a = a(*target), b = a(*diagram))
        }
        ModelError::InvalidReference { from, .. } => t!("error.invalid_reference", a = a(*from)),
        ModelError::KindChanged(id) => t!("error.kind_changed", a = a(*id)),
        ModelError::OwnershipCycle(id) => t!("error.ownership_cycle", a = a(*id)),
        ModelError::GeneralizationCycle(_) => t!("error.generalization_cycle"),
        ModelError::NotADiagram(id) => t!("error.not_a_diagram", a = a(*id)),
        ModelError::DiagramNotEmpty(id) => t!("error.diagram_not_empty", a = a(*id)),
        ModelError::EdgesAttached(_) => t!("error.edges_attached"),
        ModelError::ShapeChanged(_) => t!("error.shape_changed"),
        ModelError::EdgeCycle(_) => t!("error.edge_cycle"),
        ModelError::ReferenceCycle(id) => t!("error.reference_cycle", a = a(*id)),
        ModelError::NotFinite(_) => t!("error.not_finite"),
        ModelError::TypeTooDeep(id) => {
            t!("error.type_too_deep", a = a(*id), max = umlcraft_model::MAX_TYPE_DEPTH)
        }
        ModelError::Invalid(detail) => match detail.as_str() {
            composite::PART_OUTSIDE => t!("error.part_outside"),
            composite::PORT_OUTSIDE => t!("error.port_outside"),
            composite::PORT_ON_UNTYPED_PART => t!("error.port_on_untyped_part"),
            umlcraft_notation::activity::PIN_OUTSIDE => t!("error.pin_outside"),
            umlcraft_notation::state_edit::STUB_OUTSIDE => t!("error.stub_outside"),
            umlcraft_notation::sequence_edit::NOT_ON_LIFELINE => t!("error.not_on_lifeline"),
            umlcraft_notation::sequence_edit::LIFELINE_ENDED => t!("error.lifeline_ended"),
            umlcraft_notation::sequence_edit::NOT_ON_MESSAGE => t!("error.not_on_message"),
            umlcraft_notation::er::NOT_JOINED => t!("error.er_not_joined"),
            umlcraft_notation::er::ATTRIBUTE_OWNED => t!("error.er_attribute_owned"),
            umlcraft_notation::er::SECOND_WEAK => t!("error.er_second_weak"),
            umlcraft_notation::er::ASSOCIATIVE_IN_USE => t!("error.er_associative_in_use"),
            _ => t!("error.invalid", detail = detail),
        },
    }
    .to_string()
}

/// Why a project file could not be read or written.
pub fn format_error(err: &FormatError) -> String {
    match err {
        // The system's own text, which Windows gives in the user's language.
        FormatError::Io(err) => err.to_string(),
        FormatError::Json(err) => {
            t!("error.malformed", line = err.line(), column = err.column()).to_string()
        }
        FormatError::NotAProject => t!("error.not_a_project").to_string(),
        FormatError::TooNew { found, supported } => {
            t!("error.too_new", found = found, supported = supported).to_string()
        }
        FormatError::Migration { from, reason } => {
            t!("error.migration", from = from, reason = reason).to_string()
        }
        FormatError::Model(err) => model_error(None, err),
    }
}

/// Why an `.asta` file could not be imported.
pub fn asta_error(err: &umlcraft_asta::AstaError) -> String {
    use umlcraft_asta::AstaError;
    match err {
        AstaError::Io(err) => err.to_string(),
        AstaError::Zip(_) | AstaError::Stream(_) | AstaError::BadHeader | AstaError::Damaged => {
            t!("error.not_astah").to_string()
        }
        AstaError::NoModel => t!("error.astah_no_model").to_string(),
        AstaError::TooLarge => t!("error.astah_too_large").to_string(),
    }
}

/// Why the export to an `.asta` file failed. What Astah or Java say themselves stays as they
/// say it.
pub fn export_error(err: &umlcraft_asta_export::ExportError) -> String {
    use umlcraft_asta_export::ExportError;
    match err {
        ExportError::Io(err) => err.to_string(),
        ExportError::Launch { java, source } => {
            t!("error.java_launch", java = java.display(), reason = source).to_string()
        }
        ExportError::Astah(message) => t!("error.astah_reported", message = message).to_string(),
        ExportError::Tool { status, output } => {
            t!("error.export_tool", status = status, output = output).to_string()
        }
        ExportError::License => t!("dialog.astah_unlicensed").to_string(),
        ExportError::TimedOut { minutes } => {
            t!("error.export_timeout", minutes = minutes).to_string()
        }
        ExportError::Cancelled => t!("error.export_cancelled").to_string(),
    }
}

pub fn multiplicity_error(err: &ParseMultiplicityError) -> String {
    t!("error.multiplicity", text = err.0).to_string()
}

pub fn cardinality_error(err: &ParseCardinalityError) -> String {
    t!("error.cardinality", text = err.0).to_string()
}

/// Why an `.nsd` file could not be read as a structogram.
pub fn nsd_error(err: &umlcraft_nsd::ReadError) -> String {
    use umlcraft_nsd::ReadError;
    match err {
        ReadError::Xml { line, column, message } => {
            t!("error.nsd_xml", line = line, column = column, message = message)
        }
        ReadError::NotStructorizer => t!("error.nsd_not_structorizer"),
        ReadError::UnexpectedElement { line, column, name, parent } => t!(
            "error.nsd_unexpected_element",
            line = line,
            column = column,
            name = name,
            parent = parent
        ),
        ReadError::WrongBranches { line, column, element, .. } => {
            t!("error.nsd_wrong_branches", line = line, column = column, element = element)
        }
        ReadError::UnexpectedText { line, column, parent } => {
            t!("error.nsd_unexpected_text", line = line, column = column, parent = parent)
        }
        ReadError::Encoding(encoding) => t!("error.nsd_encoding", encoding = encoding),
        ReadError::Truncated(element) => t!("error.nsd_truncated", element = element),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::sample::order_system;

    #[test]
    fn messages_name_elements_instead_of_ids() {
        let (doc, ids) = order_system();
        let err = ModelError::HasChildren(ids.order);
        let text = model_error(Some(doc.model()), &err);
        assert!(text.contains("Order") && !text.contains(&ids.order.to_string()), "{text}");
        let text = model_error(None, &err);
        assert!(!text.contains(&ids.order.to_string()) && !text.contains("error."), "{text}");
    }

    #[test]
    fn import_and_export_errors_have_texts() {
        use umlcraft_asta::AstaError;
        use umlcraft_asta_export::ExportError;
        let texts = [
            asta_error(&AstaError::Damaged),
            asta_error(&AstaError::NoModel),
            asta_error(&AstaError::TooLarge),
            export_error(&ExportError::TimedOut { minutes: 10 }),
            export_error(&ExportError::Astah("boom".into())),
            export_error(&ExportError::Tool { status: "1".into(), output: "out".into() }),
        ];
        for text in texts {
            assert!(!text.is_empty() && !text.contains("error."), "{text}");
        }
    }
}
