//! Updates from the latest GitHub release, read and checked exactly as the
//! Tauri updater plugin (`tauri-plugin-updater` 2.14) reads and checks them.
//!
//! The Tauri version of Lueur updates itself with that plugin, from the
//! `latest.json` that `scripts/latest-json.sh` writes and the release signs
//! (`.github/workflows/release.yml`). This module reads the same manifest,
//! picks the same platform entry, verifies the same minisign signature with
//! the same public key, and installs the same files the same way, so an
//! installed Tauri app can update into this one and this one keeps updating
//! from releases made the way they are made today.
//!
//! Never an older version, nor the same one again. `latest.json` itself is
//! not signed, only each update file is, so the version it announces proves
//! nothing on its own: a doctored manifest could pair a high number with an
//! old, validly signed file. Each signature therefore carries the version it
//! was made for in its trusted comment (the release signs with
//! `--app-version`), and a file whose signed version is not the announced one
//! is refused, as the plugin does with `requireSignedVersion`. The announced
//! version must be strictly newer than this one ([`is_upgrade`]), so the
//! signed one is too.
//!
//! A version that downloads but fails to install (no write access to the
//! install folder, say) is remembered, also on disk ([`FAILED_FILE`]): later
//! launches start without downloading it again, and the running app tries it
//! again only after a few hours.
//!
//! The only network traffic this adds is HTTPS to github.com, and to the
//! GitHub download host it redirects to.

use semver::Version;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// Where the release publishes its manifest, the address the Tauri versions
/// (up to 0.18) read too: they update into this one from it.
pub const ENDPOINT: &str =
    "https://github.com/Alegzandr/lueur/releases/latest/download/latest.json";

/// The minisign public key the release signs with, base64 encoded as the
/// Tauri versions held it. Changing it would strand every installed copy.
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc5N0NBMTkxRTY1QTA0MTIKUldRU0JGcm1rYUY4ZVc0YkxHeS9xcGNURWlBYkE1bVJtSWVtbFFSenkvSTVld1d2QlNvZmthQlYK";

/// Often enough that a running app offers a new version the day it is out;
/// each check is one small HTTPS request.
pub const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A version that downloads but does not install is tens of megabytes:
/// not every hour.
pub const INSTALL_RETRY: Duration = Duration::from_secs(6 * 60 * 60);
/// After a check or a download that failed, most often for want of network.
pub const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
/// No network at login must not hold the app for long.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Holds the version that last failed to install, next to the settings.
pub const FAILED_FILE: &str = "update-failed.txt";
/// Far above any release (tens of megabytes), so a broken or hostile server
/// cannot fill the memory before the signature check refuses its file.
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

/// This build's version, the one every offered version is compared with.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// What the splash shows while an update is checked for and applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Checking,
    Downloading,
    Installing,
    Restarting,
}

/// A stage, and the download progress when the size is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub stage: Stage,
    pub percent: Option<u8>,
}

/// One platform's file in the manifest.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub url: String,
    /// The `.sig` file's content: a base64 encoded minisign signature.
    pub signature: String,
}

/// `latest.json`, in either shape the plugin reads: every platform under
/// `platforms` (what the release writes), or one `url` and `signature` for a
/// server that picked the platform itself.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub version: Version,
    platforms: Platforms,
}

#[derive(Debug, Clone)]
enum Platforms {
    Each(HashMap<String, Asset>),
    One(Asset),
}

/// The plugin's rules: `version` (or `name`) with an optional leading `v`,
/// `platforms` if present, otherwise `url` and `signature`. `notes` and
/// `pub_date` are not needed to update and are not read.
pub fn parse_manifest(json: &str) -> Result<Manifest, String> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(alias = "name")]
        version: String,
        platforms: Option<HashMap<String, Asset>>,
        url: Option<String>,
        signature: Option<String>,
    }
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("latest.json: {e}"))?;
    let version = Version::parse(raw.version.trim_start_matches('v'))
        .map_err(|e| format!("latest.json version {}: {e}", raw.version))?;
    let platforms = match raw.platforms {
        Some(each) => Platforms::Each(each),
        None => match (raw.url, raw.signature) {
            (Some(url), Some(signature)) => Platforms::One(Asset { url, signature }),
            _ => return Err("latest.json has neither platforms nor url and signature".into()),
        },
    };
    Ok(Manifest { version, platforms })
}

/// The plugin's name for this operating system.
pub fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "windows"
    }
}

/// The plugin's name for this processor.
pub fn arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "i686"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else {
        "x86_64"
    }
}

/// The bundle this copy runs from, which the plugin adds to the platform
/// name it looks for first: an AppImage on Linux, an `.app` on macOS. A
/// bare Windows executable has none.
pub fn installer() -> Option<&'static str> {
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() {
        Some("appimage")
    } else if cfg!(target_os = "macos") {
        Some("app")
    } else {
        None
    }
}

/// The platform names to look for, in the plugin's order:
/// `{os}-{arch}-{installer}`, then `{os}-{arch}`.
pub fn targets(os: &str, arch: &str, installer: Option<&str>) -> Vec<String> {
    let mut targets = Vec::new();
    if let Some(installer) = installer {
        targets.push(format!("{os}-{arch}-{installer}"));
    }
    targets.push(format!("{os}-{arch}"));
    targets
}

/// Whether `offered` may replace `current`: only a strictly newer version.
pub fn is_upgrade(current: &Version, offered: &Version) -> bool {
    offered > current
}

/// A newer release, with this platform's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// As the manifest announces it; the signature must name the same.
    pub version: String,
    pub asset: Asset,
}

/// The release to install over `current`, if the manifest offers a newer
/// one. As in the plugin, a release that is not newer is no error even
/// without a file for this platform.
pub fn pick(
    manifest: &Manifest,
    current: &Version,
    targets: &[String],
) -> Result<Option<Release>, String> {
    if !is_upgrade(current, &manifest.version) {
        return Ok(None);
    }
    let asset = match &manifest.platforms {
        Platforms::One(asset) => asset.clone(),
        Platforms::Each(each) => targets
            .iter()
            .find_map(|t| each.get(t))
            .cloned()
            .ok_or_else(|| format!("latest.json has none of {targets:?}"))?,
    };
    Ok(Some(Release {
        version: manifest.version.to_string(),
        asset,
    }))
}

/// The version a signature's trusted comment names. The Tauri CLI writes it
/// as tab-separated `key:value` pairs:
/// `timestamp:1700000000\tfile:app.tar.gz\tversion:1.2.3`.
pub fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
}

/// Whether the signed version is the announced one, compared as the plugin
/// does: as semver (so `v1.2.3` and `1.2.3` match), or literally when either
/// is not semver. A signature that names no version is refused, as with
/// `requireSignedVersion`.
fn check_signed_version(trusted_comment: &str, announced: &str) -> Result<(), String> {
    let Some(signed) = signed_version(trusted_comment) else {
        return Err("the signature names no version".into());
    };
    let matches = match (
        Version::parse(signed.trim_start_matches('v')),
        Version::parse(announced.trim_start_matches('v')),
    ) {
        (Ok(signed), Ok(announced)) => signed == announced,
        _ => signed == announced,
    };
    if matches {
        Ok(())
    } else {
        Err(format!("signed for {signed}, announced as {announced}"))
    }
}

fn base64_text(b64: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8".into())
}

/// Bytes whose signature has been checked against the release key and the
/// announced version. Only these can be installed.
pub struct Verified {
    bytes: Vec<u8>,
    version: String,
}

/// Checks `bytes` the way the plugin does before it installs anything: the
/// minisign signature against `pubkey` (both base64 encoded, as in
/// `tauri.conf.json` and `latest.json`), legacy signatures allowed as the
/// plugin allows them, then the version in the trusted comment, which is only
/// read once the signature that covers it has been checked.
pub fn verify(
    bytes: Vec<u8>,
    signature: &str,
    pubkey: &str,
    announced: &str,
) -> Result<Verified, String> {
    let key = minisign_verify::PublicKey::decode(&base64_text(pubkey)?)
        .map_err(|e| format!("public key: {e}"))?;
    let sig = minisign_verify::Signature::decode(&base64_text(signature)?)
        .map_err(|e| format!("signature: {e}"))?;
    key.verify(&bytes, &sig, true)
        .map_err(|e| format!("signature: {e}"))?;
    check_signed_version(sig.trusted_comment(), announced)?;
    Ok(Verified {
        bytes,
        version: announced.to_owned(),
    })
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .user_agent(concat!("lueur/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Reads the manifest and returns the newer release for this platform, if
/// there is one.
pub fn check(current: &Version) -> Result<Option<Release>, String> {
    let mut resp = agent(CHECK_TIMEOUT)
        .get(ENDPOINT)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?;
    // As the plugin: "no content" means no update.
    if resp.status() == 204 {
        return Ok(None);
    }
    let json = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let manifest = parse_manifest(&json)?;
    pick(&manifest, current, &targets(os(), arch(), installer()))
}

/// Downloads the release's file, reporting each percent when the size is
/// known, and verifies it.
pub fn download(
    release: &Release,
    mut on_percent: impl FnMut(Option<u8>),
) -> Result<Verified, String> {
    let resp = agent(DOWNLOAD_TIMEOUT)
        .get(&release.asset.url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| e.to_string())?;
    let total = resp.body().content_length().filter(|&t| t > 0);
    let mut reader = resp.into_body().into_reader();
    let mut bytes = Vec::with_capacity(total.unwrap_or(0).min(MAX_DOWNLOAD) as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut last = None;
    loop {
        let n = reader.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() as u64 > MAX_DOWNLOAD {
            return Err("the update file is too large".into());
        }
        let percent = total.map(|t| (bytes.len() as u64 * 100 / t).min(100) as u8);
        if percent != last {
            last = percent;
            on_percent(percent);
        }
    }
    verify(bytes, &release.asset.signature, PUBKEY, &release.version)
}

/// Installs verified bytes over this copy of the app.
///
/// On Windows the release's file is the app's executable itself: it is
/// written beside the running one and swapped in. AppImages and `.app`
/// bundles are installed as the plugin installs them.
pub fn install(update: &Verified) -> Result<(), String> {
    platform::install(&update.bytes, &update.version)
}

#[cfg(windows)]
mod platform {
    use std::io::Write;

    pub fn install(bytes: &[u8], version: &str) -> Result<(), String> {
        // The verified bytes are staged next to the executable, in a file
        // this process creates itself, so nothing can swap them before the
        // replace.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe.parent().ok_or("no install folder")?;
        let tmp = dir.join(format!("lueur.update-{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .and_then(|mut f| {
                f.write_all(bytes)?;
                f.sync_all()
            });
        let result = written
            .map_err(|e| e.to_string())
            .and_then(|()| self_replace::self_replace(&tmp).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&tmp);
        if result.is_ok() {
            crate::install::updated(version);
        }
        result
    }
}

/// The plugin's AppImage install (`install_appimage`): the current AppImage
/// is moved aside to a private folder on the same file system, the new one
/// written in its place with the same permissions, and the old one put back
/// if that fails. A gzipped tar holding an AppImage is unpacked, as the
/// plugin's default `zip` feature does; the release ships the bare AppImage.
#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    /// Where the running AppImage is: `$APPIMAGE`, which its runtime sets,
    /// otherwise this executable (the plugin's choice too).
    fn target() -> io::Result<PathBuf> {
        match std::env::var_os("APPIMAGE") {
            Some(path) => Ok(PathBuf::from(path)),
            None => std::env::current_exe()?.canonicalize(),
        }
    }

    pub fn install(bytes: &[u8], _version: &str) -> Result<(), String> {
        let target = target().map_err(|e| e.to_string())?;
        install_appimage(bytes, &target).map_err(|e| e.to_string())
    }

    fn is_gz(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x1f, 0x8b, 0x08])
    }

    fn is_elf(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x7f, b'E', b'L', b'F'])
    }

    fn install_appimage(bytes: &[u8], target: &Path) -> io::Result<()> {
        let target_dev = target.metadata()?.dev();
        // Check the payload before touching the current AppImage.
        let gz = is_gz(bytes);
        if !gz && !is_elf(bytes) {
            return Err(io::Error::other(
                "the update is neither an AppImage nor a compressed AppImage",
            ));
        }
        let locations = [
            Some(std::env::temp_dir()),
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))),
            target.parent().map(Path::to_path_buf),
        ];
        for location in locations.into_iter().flatten() {
            // A location that cannot be used moves on to the next one.
            let Ok(tmp) = tempfile::Builder::new()
                .prefix("lueur_current_app")
                .tempdir_in(&location)
            else {
                continue;
            };
            if tmp.path().metadata().map(|m| m.dev()).ok() != Some(target_dev) {
                continue;
            }
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700))?;
            let backup = tmp.path().join("current_app.AppImage");
            let permissions = std::fs::metadata(target)?.permissions();
            match std::fs::rename(target, &backup) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::CrossesDevices => continue,
                Err(e) => return Err(e),
            }
            let written = if gz {
                unpack_appimage(bytes, target)
            } else {
                std::fs::write(target, bytes)
                    .and_then(|()| std::fs::set_permissions(target, permissions))
            };
            if let Err(e) = written {
                // Something went wrong: the previous app goes back.
                std::fs::rename(&backup, target)?;
                return Err(e);
            }
            return Ok(());
        }
        Err(io::Error::other(
            "no temporary folder on the AppImage's file system",
        ))
    }

    fn unpack_appimage(bytes: &[u8], target: &Path) -> io::Result<()> {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        for mut entry in archive.entries()?.flatten() {
            let is_appimage = entry
                .path()
                .is_ok_and(|p| p.extension() == Some(std::ffi::OsStr::new("AppImage")));
            if is_appimage {
                entry.unpack(target)?;
                return Ok(());
            }
        }
        Err(io::Error::other("no AppImage in the update archive"))
    }

    /// On a pretend AppImage in a temporary folder, never the running app.
    #[cfg(test)]
    mod tests {
        use super::*;

        const NEW: &[u8] = b"\x7fELF new";

        #[test]
        fn an_appimage_is_replaced_and_keeps_its_permissions() {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("Lueur.AppImage");
            std::fs::write(&target, b"\x7fELF old").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
            install_appimage(NEW, &target).unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), NEW);
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }

        #[test]
        fn anything_but_an_appimage_leaves_the_app_alone() {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("Lueur.AppImage");
            std::fs::write(&target, b"\x7fELF old").unwrap();
            assert!(install_appimage(b"<html>", &target).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"\x7fELF old");
        }

        #[test]
        fn a_compressed_appimage_is_unpacked() {
            let mut tar = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_size(NEW.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "Lueur.AppImage", NEW).unwrap();
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
            let bytes = gz.finish().unwrap();

            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("Lueur.AppImage");
            std::fs::write(&target, b"\x7fELF old").unwrap();
            install_appimage(&bytes, &target).unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), NEW);
        }
    }
}

/// The plugin's `.app` install: the `.app.tar.gz` is unpacked beside the
/// app, on its volume, and the two bundles swapped in one step where the
/// file system can (APFS), or moved with the old one put back on failure.
/// Without the right to write there, the same swap runs with administrator
/// rights, which macOS asks the user for.
#[cfg(target_os = "macos")]
mod platform {
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    /// `/Applications/Lueur.app` for `/Applications/Lueur.app/Contents/MacOS/lueur`,
    /// otherwise the executable's folder, as the plugin resolves it.
    fn target() -> io::Result<PathBuf> {
        let exe = std::env::current_exe()?.canonicalize()?;
        let dir = exe
            .parent()
            .ok_or_else(|| io::Error::other("no install folder"))?;
        if dir.ends_with("Contents/MacOS")
            && let Some(app) = dir.parent().and_then(Path::parent)
        {
            return Ok(app.to_path_buf());
        }
        Ok(dir.to_path_buf())
    }

    pub fn install(bytes: &[u8], _version: &str) -> Result<(), String> {
        let target = target().map_err(|e| e.to_string())?;
        install_app(bytes, &target).map_err(|e| e.to_string())
    }

    fn install_app(bytes: &[u8], target: &Path) -> io::Result<()> {
        // The renames only work within one file system, so the temporary
        // folders go on the app's volume. The renames act on the install
        // path itself, so a symlink there is not followed.
        let app_dev = std::fs::symlink_metadata(target)?.dev();
        let tmp_root = [
            Some(std::env::temp_dir()),
            target.parent().map(Path::to_path_buf),
        ]
        .into_iter()
        .flatten()
        .find(|dir| dir.metadata().is_ok_and(|m| m.dev() == app_dev))
        .ok_or_else(|| io::Error::other("no temporary folder on the app's volume"))?;
        let backup_dir = tempfile::Builder::new()
            .prefix("lueur_current_app")
            .tempdir_in(&tmp_root)?;
        let extract_dir = tempfile::Builder::new()
            .prefix("lueur_updated_app")
            .tempdir_in(&tmp_root)?;

        // `Lueur.app/Contents/...` lands as `<extract_dir>/Contents/...`.
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let inner: PathBuf = entry.path()?.iter().skip(1).collect();
            let path = extract_dir.path().join(&inner);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&path)?;
        }
        // The temporary folder becomes the bundle's root, and is created
        // 0700, which would keep every other user from launching the app.
        std::fs::set_permissions(extract_dir.path(), std::fs::Permissions::from_mode(0o755))?;

        let backup = backup_dir.path().join("current_app");
        let moved = match swap_bundle(target, extract_dir.path()) {
            Err(e) if e.kind() != io::ErrorKind::PermissionDenied => {
                replace_bundle(target, extract_dir.path(), &backup)
            }
            other => other,
        };
        match moved {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                privileged_swap(target, extract_dir.path(), &backup)?;
            }
            Err(e) => {
                if let Some(kept) = keep_backup(target, backup_dir, &backup) {
                    return Err(io::Error::other(format!(
                        "{e}; the previous app is at {}",
                        kept.display()
                    )));
                }
                return Err(e);
            }
        }
        let _ = std::process::Command::new("touch").arg(target).status();
        Ok(())
    }

    /// Exchanges `target` and `staged` atomically (APFS), so the app never
    /// goes missing.
    fn swap_bundle(target: &Path, staged: &Path) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(staged.as_os_str().as_bytes())?;
        let to = CString::new(target.as_os_str().as_bytes())?;
        // SAFETY: both pointers are valid NUL-terminated strings that outlive
        // the call, and `renamex_np` only reads them.
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Moves `staged` to `target`, keeping the old app in `backup` and
    /// putting it back if the second move fails.
    fn replace_bundle(target: &Path, staged: &Path, backup: &Path) -> io::Result<()> {
        std::fs::rename(target, backup)?;
        if let Err(e) = std::fs::rename(staged, target) {
            let _ = std::fs::rename(backup, target);
            return Err(e);
        }
        Ok(())
    }

    /// The plugin's privileged install: the same swap through JavaScript
    /// for Automation, or the same moves where the volume cannot swap, run
    /// by `osascript` with administrator rights. The plugin runs the same
    /// AppleScript in process; the `osascript` tool asks for the password
    /// the same way.
    fn privileged_swap(target: &Path, staged: &Path, backup: &Path) -> io::Result<()> {
        const SWAP_JXA: &str = r#"function run(argv) {
  ObjC.import("stdio");
  return $.renamex_np(argv[0], argv[1], 2) === 0 ? "swapped" : "failed";
}"#;
        let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"));
        let swap = quote(Path::new(SWAP_JXA));
        let (target_q, staged_q, backup_q) = (quote(target), quote(staged), quote(backup));
        let command = format!(
            "case \"$(/usr/bin/osascript -l JavaScript -e {swap} {staged_q} {target_q} 2>/dev/null)\" in swapped) rm -rf {staged_q};; failed) mv -f {target_q} {backup_q} && {{ mv -f {staged_q} {target_q} || {{ mv -f {backup_q} {target_q}; exit 1; }}; }} && rm -rf {backup_q};; *) exit 1;; esac"
        );
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            command.replace('\\', r"\\").replace('"', "\\\"")
        );
        let status = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "failed to move the new app into place",
            ))
        }
    }

    /// After a failed install, where the previous app is when it did not
    /// make it back: moved beside the app, or to the home folder, as
    /// `Lueur (previous version).app`, so the system does not empty it with
    /// the temporary folder.
    fn keep_backup(target: &Path, backup_dir: tempfile::TempDir, backup: &Path) -> Option<PathBuf> {
        if std::fs::symlink_metadata(target).is_ok() || std::fs::symlink_metadata(backup).is_err() {
            return None;
        }
        let mut name = target.file_stem().unwrap_or_default().to_os_string();
        name.push(" (previous version)");
        if let Some(ext) = target.extension() {
            name.push(".");
            name.push(ext);
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let kept = [target.parent().map(Path::to_path_buf), home]
            .into_iter()
            .flatten()
            .map(|dir| dir.join(&name))
            .filter(|kept| std::fs::symlink_metadata(kept).is_err())
            .find(|kept| std::fs::rename(backup, kept).is_ok());
        match kept {
            Some(kept) => Some(kept),
            None => {
                let _ = backup_dir.keep();
                Some(backup.to_path_buf())
            }
        }
    }

    /// On a pretend bundle in a temporary folder, never the running app.
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_bundle_is_swapped_for_the_one_in_the_archive() {
            let new = b"#!/bin/sh\necho new\n";
            let mut tar = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_size(new.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "Lueur.app/Contents/MacOS/lueur", &new[..])
                .unwrap();
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
            let bytes = gz.finish().unwrap();

            let dir = tempfile::tempdir().unwrap();
            let app = dir.path().join("Lueur.app");
            let exe = app.join("Contents/MacOS/lueur");
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"old").unwrap();
            install_app(&bytes, &app).unwrap();
            assert_eq!(std::fs::read(&exe).unwrap(), new);
            let mode = std::fs::metadata(&app).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    pub fn install(_bytes: &[u8], _version: &str) -> Result<(), String> {
        Err("updates are not supported on this system".into())
    }
}

/// Why no update was installed.
#[derive(Debug)]
pub enum Failure {
    /// Checking or downloading failed: worth trying again soon.
    Fetch(String),
    /// The download is fine but could not be installed: trying again soon
    /// would only download it again for nothing.
    Install(String),
}

impl Failure {
    /// Logs the failure and says how long to wait before the next check.
    pub fn wait(&self) -> Duration {
        match self {
            Failure::Fetch(e) => {
                log::warn!("update: {e}");
                RETRY_AFTER
            }
            Failure::Install(e) => {
                log::error!("update install: {e}");
                INSTALL_RETRY
            }
        }
    }
}

/// Checks for, downloads and installs updates, and remembers the version
/// that failed to install.
pub struct Updater {
    data_dir: PathBuf,
    /// The version that failed to install in this process.
    failed: Mutex<Option<String>>,
}

impl Updater {
    /// `data_dir` is where the settings are, which holds [`FAILED_FILE`].
    pub fn new(data_dir: PathBuf) -> Self {
        Updater {
            data_dir,
            failed: Mutex::new(None),
        }
    }

    fn failed_path(&self) -> PathBuf {
        self.data_dir.join(FAILED_FILE)
    }

    /// The version that last failed to install, in this process or before.
    pub fn failed_version(&self) -> Option<String> {
        if let Some(v) = self.failed.lock().ok().and_then(|f| f.clone()) {
            return Some(v);
        }
        let text = std::fs::read_to_string(self.failed_path()).ok()?;
        Some(text.trim().to_owned()).filter(|v| !v.is_empty())
    }

    /// Remembers the version that failed to install, or forgets it with
    /// `None`.
    pub fn set_failed(&self, version: Option<&str>) {
        if let Ok(mut f) = self.failed.lock() {
            *f = version.map(str::to_owned);
        }
        let path = self.failed_path();
        let result = match version {
            Some(v) => {
                std::fs::create_dir_all(&self.data_dir).and_then(|()| std::fs::write(&path, v))
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            },
        };
        if let Err(e) = result {
            log::warn!("update: {}: {e}", path.display());
        }
    }

    /// Installs a newer release if there is one and returns its version.
    /// Blocks while it downloads: run it off the interface's thread.
    ///
    /// `splash` reports each step through `progress`, and skips a version
    /// that already failed to install: the launch must not wait on a
    /// download that cannot be used. The running app still tries it, every
    /// [`INSTALL_RETRY`].
    pub fn check_and_install(
        &self,
        splash: bool,
        mut progress: impl FnMut(Progress),
    ) -> Result<Option<String>, Failure> {
        let Some(release) = check(&current_version()).map_err(Failure::Fetch)? else {
            return Ok(None);
        };
        if splash && self.failed_version().as_deref() == Some(release.version.as_str()) {
            log::warn!("update: {} failed to install before", release.version);
            return Ok(None);
        }
        if splash {
            progress(Progress {
                stage: Stage::Downloading,
                percent: None,
            });
        }
        let verified = download(&release, |percent| {
            if splash {
                progress(Progress {
                    stage: Stage::Downloading,
                    percent,
                });
            }
        })
        .map_err(Failure::Fetch)?;
        if splash {
            progress(Progress {
                stage: Stage::Installing,
                percent: None,
            });
        }
        if let Err(e) = install(&verified) {
            self.set_failed(Some(&release.version));
            return Err(Failure::Install(e));
        }
        if self.failed_version().is_some() {
            self.set_failed(None);
        }
        log::info!("update: {} installed", release.version);
        Ok(Some(release.version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// The manifest `scripts/latest-json.sh` writes, signatures shortened.
    const LATEST: &str = r#"{
      "version": "0.19.0",
      "notes": "https://github.com/Alegzandr/lueur/releases/tag/v0.19.0",
      "pub_date": "2026-11-05T10:00:00Z",
      "platforms": {
        "windows-x86_64": {"url": "https://github.com/Alegzandr/lueur/releases/download/v0.19.0/Lueur_0.19.0_windows_x64_setup.exe", "signature": "w"},
        "linux-x86_64": {"url": "https://github.com/Alegzandr/lueur/releases/download/v0.19.0/Lueur_0.19.0_linux_x86_64.AppImage", "signature": "l"},
        "darwin-x86_64": {"url": "https://github.com/Alegzandr/lueur/releases/download/v0.19.0/Lueur_0.19.0_macos_universal.app.tar.gz", "signature": "m"},
        "darwin-aarch64": {"url": "https://github.com/Alegzandr/lueur/releases/download/v0.19.0/Lueur_0.19.0_macos_universal.app.tar.gz", "signature": "m"}
      }
    }"#;

    #[test]
    fn only_a_newer_version_is_an_update() {
        assert!(is_upgrade(&v("0.7.4"), &v("0.8.0")));
        assert!(is_upgrade(&v("0.9.0"), &v("0.10.0")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.8.0")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.7.4")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.8.0-beta.1")));
    }

    #[test]
    fn the_release_manifest_is_read_as_the_plugin_reads_it() {
        let m = parse_manifest(LATEST).unwrap();
        assert_eq!(m.version, v("0.19.0"));
        let windows = targets("windows", "x86_64", None);
        let r = pick(&m, &v("0.18.2"), &windows).unwrap().unwrap();
        assert_eq!(r.version, "0.19.0");
        assert!(r.asset.url.ends_with("windows_x64_setup.exe"));
        assert_eq!(r.asset.signature, "w");
        // The AppImage looks for `linux-x86_64-appimage` first, then falls
        // back to `linux-x86_64`, which is what the release names.
        let linux = targets("linux", "x86_64", Some("appimage"));
        assert_eq!(linux, ["linux-x86_64-appimage", "linux-x86_64"]);
        let r = pick(&m, &v("0.18.2"), &linux).unwrap().unwrap();
        assert_eq!(r.asset.signature, "l");
        let mac = targets("darwin", "aarch64", Some("app"));
        assert_eq!(
            pick(&m, &v("0.18.2"), &mac)
                .unwrap()
                .unwrap()
                .asset
                .signature,
            "m"
        );
    }

    #[test]
    fn a_release_that_is_not_newer_is_no_update_even_without_this_platform() {
        let m = parse_manifest(LATEST).unwrap();
        let none = targets("windows", "aarch64", None);
        assert_eq!(pick(&m, &v("0.19.0"), &none).unwrap(), None);
        assert_eq!(pick(&m, &v("1.0.0"), &none).unwrap(), None);
        // A newer one without this platform is an error.
        assert!(pick(&m, &v("0.18.2"), &none).is_err());
    }

    #[test]
    fn the_manifest_takes_a_leading_v_a_name_or_a_single_file() {
        let m = parse_manifest(r#"{"name": "v1.2.3", "url": "https://x/y", "signature": "s"}"#)
            .unwrap();
        assert_eq!(m.version, v("1.2.3"));
        let r = pick(&m, &v("1.0.0"), &targets("linux", "x86_64", None))
            .unwrap()
            .unwrap();
        assert_eq!(r.asset.url, "https://x/y");
        assert!(parse_manifest(r#"{"version": "1.2.3"}"#).is_err());
        assert!(parse_manifest(r#"{"version": "one", "platforms": {}}"#).is_err());
        assert!(parse_manifest("not json").is_err());
    }

    #[test]
    fn the_signed_version_is_read_from_the_trusted_comment() {
        let c = "timestamp:1793870370\tfile:payload.bin\tversion:0.19.0";
        assert_eq!(signed_version(c), Some("0.19.0"));
        assert_eq!(signed_version("timestamp:1\tfile:x"), None);
        assert!(check_signed_version(c, "0.19.0").is_ok());
        assert!(check_signed_version(c, "v0.19.0").is_ok());
        assert!(check_signed_version(c, "0.20.0").is_err());
        assert!(check_signed_version("timestamp:1\tfile:x", "0.19.0").is_err());
    }

    // A throwaway key and two signatures of `PAYLOAD` made with the Tauri
    // CLI, the signer the release uses (`tauri signer generate`, then
    // `tauri signer sign`, with and without `--app-version 0.19.0`). They
    // prove this module accepts what that signer produces, and refuses it
    // where the plugin would.
    const PAYLOAD: &[u8] = b"Lueur update test payload\n";
    const TEST_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDMwNzNCNURBNTIxNDMzNTcKUldSWE14UlMyclZ6TUtEMUY5Ri9uU1NNZVhJaW4yNU91TlR3Q2tvdnYyYWQzbGN5N3JYTENBbG4K";
    const SIGNED_0_19_0: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSWE14UlMyclZ6TVBoek13Q2VjT3Y5Nit6VlQ5SXFPT3hCOThOdE9HTEE0UllCUjJ2UHo5eXk3QXdoeGpxQnZqVnlack8zS3U2WmNvS3lzWGovN2F0WHZMVjB3bEpaSWdzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzODcwMzcwCWZpbGU6cGF5bG9hZC5iaW4JdmVyc2lvbjowLjE5LjAKaEtTZmtNSGw4VTBmdEFHNW9ST0ZYMmYvNDQrRkZEQk1KL0ZENkhZNGxXdFpUTVRwWjJvcmJNcE9ORXNVZ3F5cjJHMmdHU2dNN3dQN085Nll6Zkc3RFE9PQo=";
    const SIGNED_NO_VERSION: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSWE14UlMyclZ6TUVNUUlSVGNPbnhKQ2lrUzRXeEJhVnEvSTlWSEw4LzVyWk9MZmhib094WGFPUDVqV3BqbENrK3U1QVE3MEtoTFFvV3FURmFIVWdOL0R5Z2xSOTFFSmdFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzODcwMzcxCWZpbGU6cGF5bG9hZC5iaW4KTFIzRWFQdlBxdXZiUEhNSUFYdnhBdnpVblhMdzBiU1pHbmpXSzNPbDFPNUhPVTFFck5jN29LTWRKb3F1cEdsWW9qcmpOM1F3VUZRRlRkZmYySWU0QVE9PQo=";

    // The same payload signed by `lueur-sign`, the release's signer since
    // it left the Tauri CLI, with another key the Tauri CLI generated: the
    // updater must take its signatures exactly as it took the CLI's.
    const LUEUR_SIGN_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDk1QTE1RTI0QjVCMUIzRTcKUldUbnM3RzFKRjZobFppYk1Hd0ZOanVUU0t6endoQ09aQkdXRklRN1U0NjJXUnF2VE5BNVpRbmYK";
    const SIGNED_BY_LUEUR_SIGN: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUbnM3RzFKRjZobFE5MUVBVy9NQy9xQ05hZUg5M3pkNnJoeG1OeVFSK0pkenNjRTFOVU5ub1dKSnVSRkR4MldUcnczZ200NXdJemZxcFJQSFZUU1RZbkpSRklHcjRQV3dBPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzODc3NDIzCWZpbGU6cGF5bG9hZC5iaW4JdmVyc2lvbjowLjE5LjAKMmcxVjE2ZlhNaVpqTUZmNlZrZmFBaEhyclF2MmNzSTlkeGdaZ3ZBNjkwTXQzanBRVU1FSUNZRE9zUmJiMmxXWXh4TndJU05nbmdaZVdjZWVWdXVERGc9PQo=";

    #[test]
    fn a_file_signed_by_lueur_sign_is_accepted_as_the_tauri_cli_s_were() {
        let ok = verify(
            PAYLOAD.to_vec(),
            SIGNED_BY_LUEUR_SIGN,
            LUEUR_SIGN_PUBKEY,
            "0.19.0",
        )
        .unwrap();
        assert_eq!(ok.version, "0.19.0");
        assert!(
            verify(
                PAYLOAD.to_vec(),
                SIGNED_BY_LUEUR_SIGN,
                LUEUR_SIGN_PUBKEY,
                "0.20.0"
            )
            .is_err()
        );
        assert!(
            verify(
                PAYLOAD.to_vec(),
                SIGNED_BY_LUEUR_SIGN,
                TEST_PUBKEY,
                "0.19.0"
            )
            .is_err()
        );
    }

    #[test]
    fn a_file_signed_by_the_tauri_cli_for_the_announced_version_is_accepted() {
        let ok = verify(PAYLOAD.to_vec(), SIGNED_0_19_0, TEST_PUBKEY, "0.19.0").unwrap();
        assert_eq!(ok.bytes, PAYLOAD);
        assert_eq!(ok.version, "0.19.0");
    }

    #[test]
    fn a_signature_is_refused_for_another_version_file_or_key() {
        // A doctored manifest pairing a higher number with this file.
        assert!(verify(PAYLOAD.to_vec(), SIGNED_0_19_0, TEST_PUBKEY, "0.20.0").is_err());
        // A signature that names no version, as before `--app-version`.
        assert!(verify(PAYLOAD.to_vec(), SIGNED_NO_VERSION, TEST_PUBKEY, "0.19.0").is_err());
        // Another file.
        assert!(
            verify(
                b"Lueur update test payloaD\n".to_vec(),
                SIGNED_0_19_0,
                TEST_PUBKEY,
                "0.19.0"
            )
            .is_err()
        );
        // The release key did not make this signature.
        assert!(verify(PAYLOAD.to_vec(), SIGNED_0_19_0, PUBKEY, "0.19.0").is_err());
        assert!(verify(PAYLOAD.to_vec(), "not base64!", TEST_PUBKEY, "0.19.0").is_err());
    }

    #[test]
    fn the_release_key_decodes() {
        let key = base64_text(PUBKEY).unwrap();
        assert!(key.contains("minisign public key: 797CA191E65A0412"));
        assert!(minisign_verify::PublicKey::decode(&key).is_ok());
    }

    /// The manifest the Tauri versions read (their `tauri.conf.json`, up to
    /// 0.18): installed copies update into this one from it, so it must not
    /// move.
    #[test]
    fn the_same_manifest_as_the_tauri_versions() {
        assert_eq!(
            ENDPOINT,
            "https://github.com/Alegzandr/lueur/releases/latest/download/latest.json"
        );
    }

    #[test]
    fn this_build_names_its_platform_as_the_plugin_does() {
        let t = targets(os(), arch(), installer());
        #[cfg(all(windows, target_arch = "x86_64"))]
        assert_eq!(t, ["windows-x86_64"]);
        assert!(t.last().unwrap().contains('-'));
    }

    /// The published release, read, downloaded and verified as an update
    /// from 0.0.0 would be, without installing it. It reaches GitHub, so it
    /// only runs when asked: `cargo test -p lueur-shell -- --ignored`.
    #[test]
    #[ignore = "reaches github.com and downloads the latest release"]
    fn the_published_release_verifies() {
        let release = check(&v("0.0.0")).unwrap().expect("a release");
        let verified = download(&release, |_| {}).unwrap();
        assert_eq!(verified.version, release.version);
        assert!(verified.bytes.len() > 1_000_000);
    }

    #[test]
    fn a_failed_version_is_remembered_on_disk_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("lueur-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let updater = Updater::new(dir.clone());
        assert_eq!(updater.failed_version(), None);
        updater.set_failed(Some("0.19.0"));
        assert_eq!(
            Updater::new(dir.clone()).failed_version().as_deref(),
            Some("0.19.0")
        );
        updater.set_failed(None);
        assert_eq!(Updater::new(dir.clone()).failed_version(), None);
        assert_eq!(updater.failed_version(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
