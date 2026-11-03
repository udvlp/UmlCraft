//! Structograms (Nassi-Shneiderman diagrams) of the project: shown and edited in the main
//! window like diagrams, read from and written to Structorizer's `.nsd` files.

mod dialog;
pub mod source;
mod view;

use std::path::Path;

use gpui_kit::*;
use rust_i18n::t;
use umlcraft_nsd::{Diagram, ElementKind};

pub use view::{Allowed, NsdView};

actions!(
    nsd,
    [
        EditElement,
        MoveUp,
        MoveDown,
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        ExtendUp,
        ExtendDown,
        ToggleDisabled,
        AddBranch,
        RemoveBranch,
        // Writes the open structogram as a `.nsd` file.
        ExportStructogram,
    ]
);

/// Inserts a new element of `kind` after (or before) the selection, through the dialog.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = nsd, no_json)]
pub struct InsertElement {
    pub kind: ElementKind,
    pub after: bool,
}

/// Dyes the selected elements (white is Structorizer's "no colour").
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = nsd, no_json)]
pub struct SetColor {
    pub color: [u8; 3],
}

/// The paint box of a fresh Structorizer installation.
pub const PALETTE: [[u8; 3]; 10] = [
    [0xff, 0xff, 0xff],
    [0xff, 0x80, 0x80],
    [0xff, 0xff, 0x80],
    [0x80, 0xff, 0x80],
    [0x80, 0xff, 0xff],
    [0x00, 0x80, 0xff],
    [0xff, 0x80, 0xc0],
    [0xc0, 0xc0, 0xc0],
    [0xff, 0x80, 0x00],
    [0x80, 0x80, 0xff],
];

/// The keys of structograms.
pub fn bind_keys(cx: &mut App) {
    let c = Some("NsdView");
    cx.bind_keys([
        crate::key(cx, "enter", EditElement, c),
        crate::key(cx, "delete", crate::DeleteSelection, c),
        crate::key(cx, "secondary-c", crate::Copy, c),
        crate::key(cx, "secondary-x", crate::Cut, c),
        crate::key(cx, "shift-delete", crate::Cut, c),
        crate::key(cx, "secondary-v", crate::Paste, c),
        crate::key(cx, "escape", crate::Cancel, c),
        crate::key(cx, "up", SelectUp, c),
        crate::key(cx, "down", SelectDown, c),
        crate::key(cx, "left", SelectLeft, c),
        crate::key(cx, "right", SelectRight, c),
        crate::key(cx, "shift-up", ExtendUp, c),
        crate::key(cx, "shift-down", ExtendDown, c),
        crate::key(cx, "secondary-up", MoveUp, c),
        crate::key(cx, "secondary-down", MoveDown, c),
    ]);
}

/// An element kind's name in the UI's language.
pub fn kind_name(kind: ElementKind) -> String {
    t!(format!("nsd.kind_{}", kind.tag())).to_string()
}

/// Whether a file is a structogram, by its extension.
pub fn is_structogram(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nsd"))
}

/// Reads a Structorizer file. The error is the reason in the UI's language.
pub fn read(path: &Path) -> Result<Diagram, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| t!("error.nsd_not_utf8").to_string())?;
    umlcraft_nsd::read(&text).map_err(|e| crate::errors::nsd_error(&e))
}

/// The message for a structogram that could not be read.
pub fn open_failed() -> String {
    t!("dialog.structogram_open_failed").to_string()
}

/// Who exports: the system's user name, as Structorizer writes it.
pub(crate) fn user_name() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "???".to_owned())
}

/// Today here (not in UTC: just after midnight it is already tomorrow) as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// What exported structograms name as their origin.
fn origin() -> String {
    format!("UmlCraft {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui_kit brings a `test` attribute of its own.
    use std::path::Path;

    use super::{is_structogram, origin};

    #[test]
    fn structograms_go_by_their_extension() {
        assert!(is_structogram(Path::new("sort.nsd")));
        assert!(is_structogram(Path::new(r"C:\Diagramme\SORT.NSD")));
        assert!(!is_structogram(Path::new("shop.ucraft")));
        assert!(!is_structogram(Path::new("shop.asta")));
        assert!(!is_structogram(Path::new("nsd")));
    }

    #[test]
    fn exported_structograms_name_umlcraft_as_their_origin() {
        assert!(origin().starts_with("UmlCraft "));
    }
}
