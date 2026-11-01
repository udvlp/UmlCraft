//! The check for a newer release on GitHub. It runs only with the user's consent, at most once
//! a day, or when they ask for it in the About dialog. The request carries nothing but the user
//! agent `UmlCraft/<version>`; see `privacy_dialog` and "Privacy" in the README.

use std::sync::Once;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATEST: &str = "https://api.github.com/repos/udvlp/UmlCraft/releases/latest";
const RELEASES: &str = "https://github.com/udvlp/UmlCraft/releases";
/// How long an automatic check waits after the last one.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The version running. Debug builds take `UMLCRAFT_PRETEND_VERSION` instead, if set, to try
/// the check out.
pub fn current_version() -> String {
    let pretend = std::env::var("UMLCRAFT_PRETEND_VERSION").ok().filter(|v| !v.is_empty());
    match pretend {
        Some(version) if cfg!(debug_assertions) => version,
        _ => env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// The page of the release of `version`.
pub fn release_url(version: &str) -> String {
    format!("{RELEASES}/tag/v{version}")
}

/// The version tagged `tag` (`v0.15.3` or `0.15.3`) if it is a release newer than `current`.
/// Pre-releases and tags that are no version give `None`.
pub fn newer(tag: &str, current: &str) -> Option<semver::Version> {
    let parse = |v: &str| semver::Version::parse(v.trim().trim_start_matches('v')).ok();
    let (version, current) = (parse(tag)?, parse(current)?);
    (version.pre.is_empty() && version > current).then_some(version)
}

/// Asks GitHub for the tag of the latest release. It blocks for up to 10 seconds: call it off
/// the main thread.
pub fn fetch_latest() -> Result<String, String> {
    // ureq leaves the choice of rustls' cryptography to the application.
    static PROVIDER: Once = Once::new();
    PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(tls)
        .build()
        .into();
    let mut response = agent
        .get(LATEST)
        .header("User-Agent", format!("UmlCraft/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    tag_of(&body)
}

/// The `tag_name` of GitHub's answer about a release.
fn tag_of(json: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    value
        .get("tag_name")
        .and_then(|t| t.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "no tag_name".to_owned())
}

/// Seconds since the Unix epoch, for the time of the last check.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_newer_releases_count() {
        assert_eq!(newer("v0.15.3", "0.15.2").map(|v| v.to_string()), Some("0.15.3".into()));
        assert_eq!(newer("0.16.0", "0.15.2").map(|v| v.to_string()), Some("0.16.0".into()));
        assert_eq!(newer("v0.15.2", "0.15.2"), None);
        assert_eq!(newer("v0.15.1", "0.15.2"), None);
        assert_eq!(newer("v0.16.0-beta.1", "0.15.2"), None);
        assert_eq!(newer("latest", "0.15.2"), None);
        assert_eq!(newer("v0.15.3", "rubbish"), None);
    }

    #[test]
    fn the_tag_comes_from_githubs_answer() {
        let json = r#"{"tag_name": "v0.15.3", "name": "UmlCraft 0.15.3", "draft": false}"#;
        assert_eq!(tag_of(json), Ok("v0.15.3".into()));
        assert!(tag_of(r#"{"message": "Not Found"}"#).is_err());
        assert!(tag_of("<html>").is_err());
    }

    /// Asks GitHub for real: `cargo test -p umlcraft-app reaches_github -- --ignored`.
    #[test]
    #[ignore]
    fn reaches_github() {
        let tag = fetch_latest().unwrap();
        assert!(semver::Version::parse(tag.trim_start_matches('v')).is_ok(), "{tag}");
    }
}
