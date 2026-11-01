//! Small per-user settings, stored as JSON in `UmlCraft/settings.json` in the platform's folder
//! for them: `%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
//! `$XDG_CONFIG_HOME` (else `~/.config`) elsewhere.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 8;

/// The languages of the user interface, each with its name in itself.
pub const LANGUAGES: [(&str, &str); 2] = [("de", "Deutsch"), ("en", "English")];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Most recently used project files, newest first.
    pub recent: Vec<PathBuf>,
    /// `None` follows the system's language.
    pub language: Option<String>,
    pub dark: Option<bool>,
    /// Draw members' visibility in a column of its own, so all names of a compartment start
    /// in one line. Off by default: members then look as in Astah.
    pub visibility_column: bool,
    /// Mark abstract classes with the stereotype «abstract» above their name instead of UML's
    /// `{abstract}` below it.
    pub abstract_stereotype: bool,
    /// The ribbon shows only its tabs.
    pub ribbon_collapsed: bool,
}

impl Settings {
    fn file() -> Option<PathBuf> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let base = if cfg!(windows) {
            var("APPDATA")
        } else if cfg!(target_os = "macos") {
            var("HOME").map(|h| h.join("Library").join("Application Support"))
        } else {
            var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
        }?;
        Some(base.join("UmlCraft").join("settings.json"))
    }

    /// Loads the settings; a missing or broken file gives the defaults.
    pub fn load() -> Self {
        Self::file()
            .and_then(|f| fs::read_to_string(f).ok())
            .map_or_else(Self::default, |json| Self::from_json(&json))
    }

    /// Reads each setting on its own, so one that is wrong (`"dark": "yes"`) falls back to its
    /// default without taking the others, the recent files above all, along.
    fn from_json(json: &str) -> Self {
        let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(json) else {
            return Self::default();
        };
        fn field<T: serde::de::DeserializeOwned + Default>(
            fields: &serde_json::Map<String, serde_json::Value>,
            name: &str,
        ) -> T {
            fields
                .get(name)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default()
        }
        let recent = match fields.get("recent") {
            Some(serde_json::Value::Array(paths)) => {
                paths.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect()
            }
            _ => Vec::new(),
        };
        Self {
            recent,
            language: field(&fields, "language"),
            dark: field(&fields, "dark"),
            visibility_column: field(&fields, "visibilityColumn"),
            abstract_stereotype: field(&fields, "abstractStereotype"),
            ribbon_collapsed: field(&fields, "ribbonCollapsed"),
        }
    }

    /// Best effort: a failure to write settings must never disturb the user's work. Written
    /// to a file beside and renamed, so a crash never leaves half a file.
    pub fn save(&self) {
        let Some(file) = Self::file() else { return };
        if let Some(dir) = file.parent()
            && fs::create_dir_all(dir).is_ok()
            && let Ok(json) = serde_json::to_string_pretty(self)
        {
            let partial = file.with_extension("json.tmp");
            if fs::write(&partial, json).is_ok() && fs::rename(&partial, &file).is_err() {
                let _ = fs::remove_file(&partial);
            }
        }
    }

    /// Adds `path` to the recent files as saved now, so that another UmlCraft window's recent
    /// files are kept, and saves them.
    pub fn remember(&mut self, path: &Path) {
        if let Some(file) = Self::file()
            && let Ok(json) = fs::read_to_string(file)
        {
            let saved = Self::from_json(&json).recent;
            if !saved.is_empty() {
                self.recent = saved;
            }
        }
        self.add_recent(path);
        self.save();
    }

    /// How diagrams are drawn, as far as the settings decide it.
    pub fn display(&self) -> crate::canvas::Display {
        crate::canvas::Display {
            visibility_column: self.visibility_column,
            abstract_stereotype: self.abstract_stereotype,
        }
    }

    /// The language to show: the chosen one, else the system's.
    pub fn ui_language(&self) -> String {
        self.language.clone().unwrap_or_else(|| system_language().to_owned())
    }

    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// Takes `path` off the recent files, e.g. once it no longer opens. Whether it was there.
    pub fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.recent.len();
        self.recent.retain(|p| p != path);
        self.recent.len() != before
    }
}

/// The system's language if UmlCraft speaks it, else English.
fn system_language() -> &'static str {
    language_of(&sys_locale::get_locale().unwrap_or_default())
}

/// The language of the user interface for a locale such as `de-AT`.
fn language_of(locale: &str) -> &'static str {
    LANGUAGES.iter().map(|&(code, _)| code).find(|code| locale.starts_with(code)).unwrap_or("en")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_unique_newest_first_and_bounded() {
        let mut s = Settings::default();
        for i in 0..10 {
            s.add_recent(Path::new(&format!("p{i}.ucraft")));
        }
        s.add_recent(Path::new("p5.ucraft"));
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent[0], Path::new("p5.ucraft"));
        assert_eq!(s.recent.iter().filter(|p| *p == Path::new("p5.ucraft")).count(), 1);
    }

    #[test]
    fn a_wrong_setting_keeps_the_others() {
        let s = Settings::from_json(
            r#"{"recent": ["a.ucraft", 3, "b.ucraft"], "dark": "yes", "language": "de", "visibilityColumn": true}"#,
        );
        assert_eq!(s.recent, [Path::new("a.ucraft"), Path::new("b.ucraft")]);
        assert_eq!((s.dark, s.language.as_deref(), s.visibility_column), (None, Some("de"), true));
        assert_eq!(Settings::from_json("not json"), Settings::default());
        let round = Settings::from_json(&serde_json::to_string(&s).unwrap());
        assert_eq!(round, s);
    }

    #[test]
    fn system_locales_map_to_a_known_language() {
        assert_eq!(language_of("de-AT"), "de");
        assert_eq!(language_of("de"), "de");
        assert_eq!(language_of("en-US"), "en");
        assert_eq!(language_of("fr-FR"), "en");
        assert_eq!(language_of(""), "en");
    }
}
