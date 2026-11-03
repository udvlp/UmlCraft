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
    /// `None` follows the system's appearance.
    pub dark: Option<bool>,
    /// Draw members' visibility in a column of its own, so all names of a compartment start
    /// in one line. Off by default: members then look as in Astah.
    pub visibility_column: bool,
    /// Mark abstract classes with the stereotype «abstract» above their name instead of UML's
    /// `{abstract}` below it.
    pub abstract_stereotype: bool,
    /// End read-only attributes in UML's `{readOnly}`. Off by default.
    pub read_only_constraint: bool,
    /// The language of new structograms' keywords (`de`, `en`); `None` follows the user
    /// interface.
    pub structogram_keywords: Option<String>,
    /// The ribbon shows only its tabs.
    pub ribbon_collapsed: bool,
    /// Whether UmlCraft may ask GitHub for new versions; `None` until the user answered.
    pub update_check: Option<bool>,
    /// When it last asked, in seconds since the Unix epoch.
    pub last_update_check: Option<u64>,
    /// The latest release it found, e.g. `0.15.3`.
    pub latest_version: Option<String>,
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
            read_only_constraint: field(&fields, "readOnlyConstraint"),
            structogram_keywords: field(&fields, "structogramKeywords"),
            ribbon_collapsed: field(&fields, "ribbonCollapsed"),
            update_check: field(&fields, "updateCheck"),
            last_update_check: field(&fields, "lastUpdateCheck"),
            latest_version: field(&fields, "latestVersion"),
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
            read_only_constraint: self.read_only_constraint,
        }
    }

    /// The keywords new structograms are written with: German or English as chosen, else as
    /// the user interface.
    pub fn keywords(&self) -> umlcraft_nsd::Keywords {
        let language = self.structogram_keywords.clone().unwrap_or_else(|| self.ui_language());
        match language.starts_with("de") {
            true => umlcraft_nsd::Keywords::German,
            false => umlcraft_nsd::Keywords::English,
        }
    }

    /// The language to show: the chosen one, else the system's.
    pub fn ui_language(&self) -> String {
        self.language.clone().unwrap_or_else(|| system_language().to_owned())
    }

    /// Whether to ask GitHub for new versions now (`now` in seconds since the Unix epoch): if
    /// the user agreed and the last check is a day ago.
    pub fn update_due(&self, now: u64) -> bool {
        let interval = crate::update::INTERVAL.as_secs();
        self.update_check == Some(true)
            && self.last_update_check.is_none_or(|last| now >= last.saturating_add(interval))
    }

    /// The release found last, if it is newer than the version running.
    pub fn available_update(&self) -> Option<String> {
        let latest = self.latest_version.as_deref()?;
        crate::update::newer(latest, &crate::update::current_version()).map(|v| v.to_string())
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
    fn structogram_keywords_follow_the_interface_unless_chosen() {
        use umlcraft_nsd::Keywords;
        let mut s = Settings { language: Some("de".into()), ..Settings::default() };
        assert_eq!(s.keywords(), Keywords::German);
        s.structogram_keywords = Some("en".into());
        assert_eq!(s.keywords(), Keywords::English);
        s.language = Some("en".into());
        s.structogram_keywords = None;
        assert_eq!(s.keywords(), Keywords::English);
        let s = Settings::from_json(r#"{"language": "en", "structogramKeywords": "de"}"#);
        assert_eq!(s.keywords(), Keywords::German);
    }

    #[test]
    fn updates_are_checked_once_a_day_and_only_when_agreed() {
        let day = crate::update::INTERVAL.as_secs();
        let s = Settings::from_json(r#"{"updateCheck": true, "lastUpdateCheck": 1000}"#);
        assert!(!s.update_due(1000 + day - 1));
        assert!(s.update_due(1000 + day));
        assert!(Settings { update_check: Some(true), ..Settings::default() }.update_due(5));
        assert!(!Settings { update_check: Some(false), ..Settings::default() }.update_due(5));
        assert!(!Settings::default().update_due(5));
        let found = |v: &str| Settings { latest_version: Some(v.into()), ..Settings::default() };
        assert_eq!(found("999.0.0").available_update().as_deref(), Some("999.0.0"));
        assert_eq!(found("0.1.0").available_update(), None);
        assert_eq!(Settings::default().available_update(), None);
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
