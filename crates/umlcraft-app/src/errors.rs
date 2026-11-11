//! Error messages in the language of the UI, naming elements instead of showing their ids.

use rust_i18n::t;
use umlcraft_format::FormatError;
use umlcraft_model::{
    Id, Model, ModelError, ParseCardinalityError, ParseMultiplicityError, Refusal,
};
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
    kind_in(type_name, &rust_i18n::locale())
}

fn kind_in(type_name: &str, locale: &str) -> String {
    t!(format!("kind.{type_name}"), locale = locale).to_string()
}

/// Why an edit the user asked for was refused, in `locale`.
fn refusal(refusal: &Refusal, locale: &str) -> String {
    let kind = |type_name: &str| kind_in(type_name, locale);
    match refusal {
        Refusal::AlreadyShown => t!("error.already_shown", locale = locale),
        Refusal::OtherInteraction => t!("error.other_interaction", locale = locale),
        Refusal::CannotShow(what) => t!("error.cannot_show", locale = locale, what = kind(what)),
        Refusal::CannotConnect { from, to, .. } => {
            t!("error.cannot_connect", locale = locale, source = kind(from), target = kind(to))
        }
        Refusal::CannotPaste(what) => t!("error.cannot_paste", locale = locale, what = kind(what)),
        Refusal::ConnectorSameEnds => t!("error.connector_same_ends", locale = locale),
        Refusal::ConnectorNoClass => t!("error.connector_no_class", locale = locale),
        Refusal::ConnectorCannot(what) => {
            t!("error.connector_cannot", locale = locale, what = kind(what))
        }
        Refusal::CreatedAlready => t!("error.created_already", locale = locale),
        Refusal::NoRoom => t!("error.no_room", locale = locale),
        Refusal::MessagesConnectLifelines => {
            t!("error.messages_connect_lifelines", locale = locale)
        }
        Refusal::PartitionsStacked => t!("error.partitions_stacked", locale = locale),
        Refusal::SequenceNotArranged => t!("error.sequence_not_arranged", locale = locale),
        Refusal::EmptyName => t!("error.empty_name", locale = locale),
        Refusal::BindingWithoutTemplate => t!("error.binding_without_template", locale = locale),
        Refusal::HasAssociationClass => t!("error.has_association_class", locale = locale),
        Refusal::MemberNoName(text) => {
            t!("error.member_no_name", locale = locale, text = format!("{text:?}"))
        }
        Refusal::MemberMultiplicity(text) => {
            t!("error.member_multiplicity", locale = locale, text = format!("{text:?}"))
        }
        Refusal::MemberMissing { missing, .. } => {
            t!("error.member_missing", locale = locale, missing = missing)
        }
        Refusal::MemberUnexpected(_) => t!("error.member_unexpected", locale = locale),
    }
    .to_string()
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
        ModelError::StructogramTooDeep(id) => {
            t!("error.structogram_too_deep", a = a(*id), max = umlcraft_nsd::read::MAX_DEPTH)
        }
        ModelError::Refused(r) => refusal(r, &rust_i18n::locale()).into(),
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
            detail => {
                // A check of the model's own consistency: what failed is for the log, in
                // English; the user learns that the edit was not made.
                eprintln!("umlcraft: model inconsistent: {detail}");
                t!("error.invalid")
            }
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
        ReadError::TooDeep { line, column, max } => {
            t!("error.nsd_too_deep", line = line, column = column, max = max)
        }
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
    fn refusals_are_told_in_the_users_language() {
        use umlcraft_notation::member::{parse_attribute, parse_operation};
        let refusals = [
            Refusal::AlreadyShown,
            Refusal::OtherInteraction,
            Refusal::CannotShow("class"),
            Refusal::CannotConnect { tool: "transition", from: "class", to: "actor" },
            Refusal::CannotPaste("state"),
            Refusal::ConnectorSameEnds,
            Refusal::ConnectorNoClass,
            Refusal::ConnectorCannot("class"),
            Refusal::CreatedAlready,
            Refusal::NoRoom,
            Refusal::MessagesConnectLifelines,
            Refusal::PartitionsStacked,
            Refusal::SequenceNotArranged,
            Refusal::EmptyName,
            Refusal::BindingWithoutTemplate,
            Refusal::HasAssociationClass,
            parse_attribute("x : int [a..b]").unwrap_err().0,
            parse_attribute(": int").unwrap_err().0,
            parse_operation("op(").unwrap_err().0,
            parse_operation("op() int").unwrap_err().0,
        ];
        // Each locale asked for by name: the locale of the process stays as it is for the
        // tests that run alongside.
        for r in &refusals {
            for locale in ["de", "en"] {
                let text = refusal(r, locale);
                assert!(!text.contains("error.") && !text.contains("kind."), "{r:?}: {text}");
            }
        }
        // What is no refusal is a check of the model; its English stays out of the message.
        let text = model_error(None, &ModelError::Invalid("the name index is out of date".into()));
        assert!(!text.contains("index"), "{text}");
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
