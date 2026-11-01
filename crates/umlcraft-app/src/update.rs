//! The check for a newer release on GitHub. It runs only with the user's consent, at most once
//! a day, or when they ask for it in the About dialog. The request carries nothing but the user
//! agent `UmlCraft/<version>`; see `privacy_dialog` and "Privacy" in the README.
//!
//! Where UmlCraft can replace itself (the Windows installer, an AppImage, the macOS app), a click
//! on the update downloads the new release's package from GitHub, checks it against the
//! release's `SHA256SUMS.txt` and installs it once UmlCraft has quit.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Once, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

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

/// The file `name` attached to the release of `version`.
fn download_url(version: &str, name: &str) -> String {
    format!("{RELEASES}/download/v{version}/{name}")
}

/// The version tagged `tag` (`v0.15.3` or `0.15.3`) if it is a release newer than `current`.
/// Pre-releases and tags that are no version give `None`.
pub fn newer(tag: &str, current: &str) -> Option<semver::Version> {
    let parse = |v: &str| semver::Version::parse(v.trim().trim_start_matches('v')).ok();
    let (version, current) = (parse(tag)?, parse(current)?);
    (version.pre.is_empty() && version > current).then_some(version)
}

/// An HTTPS client that gives up after `timeout` in all.
fn agent(timeout: Option<Duration>) -> ureq::Agent {
    // ureq leaves the choice of rustls' cryptography to the application.
    static PROVIDER: Once = Once::new();
    PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(10)))
        .tls_config(tls)
        .build()
        .into()
}

/// A GET request that names UmlCraft and its version, and nothing else.
fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    agent
        .get(url)
        .header("User-Agent", format!("UmlCraft/{}", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| e.to_string())
}

/// Asks GitHub for the tag of the latest release. It blocks for up to 10 seconds: call it off
/// the main thread.
pub fn fetch_latest() -> Result<String, String> {
    let agent = agent(Some(Duration::from_secs(10)));
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

/// How UmlCraft was installed, where it can install a new release itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// By the Windows installer, for the current user or for all users.
    WindowsSetup { all_users: bool },
    /// As an AppImage: the file.
    AppImage(PathBuf),
    /// As the macOS app: the bundle, `…/UmlCraft.app`.
    MacApp(PathBuf),
}

/// How the running UmlCraft was installed, if it can update itself; looked up once.
pub fn install() -> Option<&'static Install> {
    static INSTALL: OnceLock<Option<Install>> = OnceLock::new();
    INSTALL.get_or_init(Install::detect).as_ref()
}

impl Install {
    /// Asks the system. The zip and tar.gz archives, the Linux packages, Flatpak and Snap, and
    /// a folder UmlCraft may not write to give `None`: there the release page has to do.
    fn detect() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let install = if cfg!(windows) {
            // Inno Setup leaves its uninstaller beside the program.
            let dir = exe.parent()?;
            if !dir.join("unins000.exe").is_file() {
                return None;
            }
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            Install::WindowsSetup { all_users: !local.is_some_and(|l| is_within(dir, &l)) }
        } else if cfg!(target_os = "macos") {
            Install::MacApp(bundle_of(&exe)?)
        } else {
            // Set by the AppImage's runtime to the file it runs from.
            Install::AppImage(PathBuf::from(std::env::var_os("APPIMAGE")?))
        };
        install.writable().then_some(install)
    }

    /// Whether UmlCraft may replace itself where it is. The Windows installer asks for the
    /// rights it needs itself.
    fn writable(&self) -> bool {
        let dir = match self {
            Install::WindowsSetup { .. } => return true,
            Install::AppImage(file) | Install::MacApp(file) => file.parent(),
        };
        let Some(dir) = dir else { return false };
        let probe = dir.join(".umlcraft-update-probe");
        let ok = fs::File::create_new(&probe).is_ok();
        let _ = fs::remove_file(&probe);
        ok
    }

    /// The name of the package of `version` for this kind of installation, as the release
    /// workflow names it.
    pub fn asset(&self, version: &str) -> String {
        match self {
            Install::WindowsSetup { .. } => format!("UmlCraft-{version}-windows-x64-setup.exe"),
            Install::AppImage(_) => format!("UmlCraft-{version}-linux-x64.AppImage"),
            Install::MacApp(_) => format!("UmlCraft-{version}-macos-universal.dmg"),
        }
    }

    /// Where the package goes: beside an AppImage, so that it can replace it by renaming, else
    /// into a folder of the system's temporary files.
    fn download_dir(&self) -> Option<PathBuf> {
        match self {
            Install::AppImage(file) => file.parent().map(Path::to_path_buf),
            _ => Some(std::env::temp_dir().join("UmlCraft-update")),
        }
    }
}

/// Whether `dir` lies in `base`, ignoring case as Windows does.
fn is_within(dir: &Path, base: &Path) -> bool {
    let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    lower(dir).starts_with(lower(base))
}

/// The app bundle the program `exe` belongs to: `X.app` of `X.app/Contents/MacOS/exe`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    let in_contents = exe.parent()?.ends_with("Contents/MacOS");
    (is_app && in_contents).then(|| bundle.to_path_buf())
}

/// The checksum `SHA256SUMS.txt` lists for the file `name`, in lowercase hex.
fn checksum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        // sha256sum marks files it read in binary mode with a `*`.
        let file = file.trim_start().trim_start_matches('*');
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 checksum of the file at `path`, in lowercase hex.
fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut fs::File::open(path)?, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

/// Downloads the package of `version` for `install` and checks it against the release's
/// checksums, reporting the bytes so far and in all as they come. A package downloaded and
/// checked before is taken again. It blocks for minutes at most: call it off the main thread.
pub fn download(
    install: &Install,
    version: &str,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let name = install.asset(version);
    let dir = install.download_dir().ok_or("no folder for the download")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let agent = agent(Some(Duration::from_secs(30 * 60)));
    let mut sums = get(&agent, &download_url(version, "SHA256SUMS.txt"))?;
    let sums = sums.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let expected = checksum_of(&sums, &name).ok_or("the release lists no checksum for it")?;
    let file = dir.join(&name);
    if file.is_file() && sha256_of(&file).is_ok_and(|hash| hash == expected) {
        return Ok(file);
    }

    let part = dir.join(format!("{name}.part"));
    let mut response = get(&agent, &download_url(version, &name))?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();
    let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut done = 0;
    let copied = loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) => break Err(e.to_string()),
        };
        hasher.update(&buffer[..n]);
        if let Err(e) = out.write_all(&buffer[..n]) {
            break Err(e.to_string());
        }
        done += n as u64;
        progress(done, total);
    };
    drop(out);
    let checked = copied.and_then(|()| {
        if hex(&hasher.finalize()) == expected {
            Ok(())
        } else {
            Err("the download does not match its checksum".to_owned())
        }
    });
    if let Err(e) = checked.and_then(|()| {
        make_executable(&part)?;
        fs::rename(&part, &file).map_err(|e| e.to_string())
    }) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    Ok(file)
}

/// Lets an AppImage run; the other packages need nothing.
fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(())
}

/// Starts installing the downloaded `file`; UmlCraft has to quit right after.
///
/// - Windows: the installer runs without questions where UmlCraft is installed, closes what
///   still uses its files and starts UmlCraft again (`/RELAUNCH`, see `umlcraft.iss`).
/// - AppImage: the new file takes the old one's place, and a shell starts it once this
///   UmlCraft has quit.
/// - macOS: a shell script waits for UmlCraft to quit, puts the app from the disk image in
///   place of the bundle and starts it.
pub fn apply(install: &Install, file: &Path) -> Result<(), String> {
    let pid = std::process::id().to_string();
    let started = match install {
        Install::WindowsSetup { all_users } => Command::new(file)
            .args(["/SILENT", "/SP-", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
            .arg("/RELAUNCH")
            .arg(if *all_users { "/ALLUSERS" } else { "/CURRENTUSER" })
            .spawn(),
        Install::AppImage(target) => {
            replace_file(file, target)?;
            Command::new("/bin/sh")
                .arg("-c")
                .arg(RELAUNCH_SCRIPT)
                .arg("sh")
                .arg(&pid)
                .arg(target)
                .spawn()
        }
        Install::MacApp(bundle) => {
            let script = file.with_file_name("apply.sh");
            fs::write(&script, MAC_SCRIPT).map_err(|e| e.to_string())?;
            Command::new("/bin/sh").arg(&script).arg(&pid).arg(file).arg(bundle).spawn()
        }
    };
    started.map(drop).map_err(|e| e.to_string())
}

/// Puts `file` in place of `target`. A running program keeps its old file on Linux.
fn replace_file(file: &Path, target: &Path) -> Result<(), String> {
    if file == target {
        return Ok(());
    }
    fs::rename(file, target).map_err(|e| e.to_string())
}

/// `sh -c RELAUNCH_SCRIPT sh <pid> <program>`: starts the program once the process has ended.
const RELAUNCH_SCRIPT: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#;

/// `sh apply.sh <pid> <dmg> <bundle>`: once the process has ended, replaces the app bundle by
/// the `UmlCraft.app` in the disk image and starts it. If anything fails, the old bundle stays
/// and starts again. `UMLCRAFT_NO_OPEN` leaves out the start, for the test.
const MAC_SCRIPT: &str = r#"#!/bin/sh
pid=$1 dmg=$2 app=$3
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
mnt=$(/usr/bin/mktemp -d) || exit 1
if /usr/bin/hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$dmg" >/dev/null
then
  /bin/rm -rf "$app.new" "$app.old"
  if /usr/bin/ditto "$mnt/UmlCraft.app" "$app.new"; then
    if /bin/mv "$app" "$app.old"; then
      if /bin/mv "$app.new" "$app"; then
        /bin/rm -rf "$app.old"
      else
        /bin/mv "$app.old" "$app"
      fi
    fi
  fi
  /bin/rm -rf "$app.new"
  /usr/bin/hdiutil detach "$mnt" >/dev/null
fi
/bin/rmdir "$mnt"
[ -n "$UMLCRAFT_NO_OPEN" ] || /usr/bin/open "$app"
"#;

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

    #[test]
    fn packages_are_named_as_the_release_workflow_names_them() {
        let setup = Install::WindowsSetup { all_users: false };
        assert_eq!(setup.asset("0.16.0"), "UmlCraft-0.16.0-windows-x64-setup.exe");
        let appimage = Install::AppImage("/home/u/UmlCraft.AppImage".into());
        assert_eq!(appimage.asset("0.16.0"), "UmlCraft-0.16.0-linux-x64.AppImage");
        let mac = Install::MacApp("/Applications/UmlCraft.app".into());
        assert_eq!(mac.asset("0.16.0"), "UmlCraft-0.16.0-macos-universal.dmg");
        assert_eq!(
            download_url("0.16.0", "SHA256SUMS.txt"),
            "https://github.com/udvlp/UmlCraft/releases/download/v0.16.0/SHA256SUMS.txt"
        );
        assert_eq!(appimage.download_dir(), Some(PathBuf::from("/home/u")));
    }

    #[test]
    fn checksums_come_from_sha256sums() {
        let sums = "ABC123  UmlCraft-0.16.0-windows-x64.zip\n\
                    def456 *UmlCraft-0.16.0-windows-x64-setup.exe\n";
        let setup = "UmlCraft-0.16.0-windows-x64-setup.exe";
        assert_eq!(checksum_of(sums, setup).as_deref(), Some("def456"));
        assert_eq!(checksum_of(sums, "UmlCraft-0.16.0-windows-x64.zip").as_deref(), Some("abc123"));
        assert_eq!(checksum_of(sums, "UmlCraft-0.16.0-linux-x64.AppImage"), None);
        assert_eq!(&hex(&Sha256::digest(b"abc"))[..8], "ba7816bf");
    }

    #[test]
    fn the_install_folder_tells_for_whom() {
        let local = Path::new(r"C:\Users\u\AppData\Local");
        assert!(is_within(Path::new(r"C:\Users\u\AppData\Local\Programs\UmlCraft"), local));
        assert!(is_within(Path::new(r"c:\users\U\appdata\local\Programs\UmlCraft"), local));
        assert!(!is_within(Path::new(r"C:\Program Files\UmlCraft"), local));
        assert!(!is_within(Path::new(r"C:\Users\u\AppData\Local2\UmlCraft"), local));
    }

    #[test]
    fn the_bundle_is_found_from_the_program() {
        let exe = Path::new("/Applications/UmlCraft.app/Contents/MacOS/umlcraft");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/UmlCraft.app")));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/umlcraft")), None);
        assert_eq!(bundle_of(Path::new("/x/UmlCraft.app/Contents/Resources/umlcraft")), None);
    }

    /// A folder of its own for a test, empty.
    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umlcraft-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn a_new_appimage_takes_the_place_of_the_old() {
        let dir = scratch("appimage");
        let (new, old) = (dir.join("new.AppImage"), dir.join("UmlCraft.AppImage"));
        fs::write(&new, "new").unwrap();
        fs::write(&old, "old").unwrap();
        make_executable(&new).unwrap();
        replace_file(&new, &old).unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "new");
        assert!(!new.exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&old).unwrap().permissions().mode() & 0o777, 0o755);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs the script against a disk image made for the test, with an app bundle as stand-in.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_script_puts_the_new_app_in_place() {
        let dir = scratch("mac-update");
        let source = dir.join("source/UmlCraft.app/Contents");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("version"), "new").unwrap();
        let app = dir.join("Applications/UmlCraft.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/version"), "old").unwrap();
        let dmg = dir.join("UmlCraft.dmg");
        let made = Command::new("hdiutil")
            .args(["create", "-volname", "UmlCraft", "-format", "UDZO", "-srcfolder"])
            .arg(dir.join("source"))
            .arg(&dmg)
            .status()
            .unwrap();
        assert!(made.success());
        let script = dir.join("apply.sh");
        fs::write(&script, MAC_SCRIPT).unwrap();
        // A process that has ended already.
        let mut gone = Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        let ran = Command::new("/bin/sh")
            .arg(&script)
            .arg(gone.id().to_string())
            .arg(&dmg)
            .arg(&app)
            .env("UMLCRAFT_NO_OPEN", "1")
            .status()
            .unwrap();
        assert!(ran.success());
        assert_eq!(fs::read_to_string(app.join("Contents/version")).unwrap(), "new");
        assert!(!dir.join("Applications/UmlCraft.app.new").exists());
        assert!(!dir.join("Applications/UmlCraft.app.old").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Asks GitHub for real: `cargo test -p umlcraft-app reaches_github -- --ignored`.
    #[test]
    #[ignore]
    fn reaches_github() {
        let tag = fetch_latest().unwrap();
        assert!(semver::Version::parse(tag.trim_start_matches('v')).is_ok(), "{tag}");
    }

    /// Downloads and checks this system's package of the latest release for real:
    /// `cargo test -p umlcraft-app downloads_the_package -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_the_package() {
        let version = fetch_latest().unwrap().trim_start_matches('v').to_owned();
        let install = if cfg!(windows) {
            Install::WindowsSetup { all_users: false }
        } else if cfg!(target_os = "macos") {
            Install::MacApp("/Applications/UmlCraft.app".into())
        } else {
            Install::AppImage(std::env::temp_dir().join("UmlCraft.AppImage"))
        };
        let mut last = (0, None);
        let file = download(&install, &version, |done, total| last = (done, total)).unwrap();
        let size = fs::metadata(&file).unwrap().len();
        assert_eq!(last, (size, Some(size)));
        // The second time it takes the file checked before.
        let mut called = false;
        assert_eq!(download(&install, &version, |_, _| called = true).unwrap(), file);
        assert!(!called);
        fs::remove_file(&file).unwrap();
    }
}
