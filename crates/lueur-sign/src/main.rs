//! `lueur-sign --app-version <version> <file>…`
//!
//! Writes `<file>.sig` beside each update file, signed with the key in
//! `TAURI_SIGNING_PRIVATE_KEY` (and its password in
//! `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, if it has one): the release's
//! secrets, under the names the Tauri CLI read them by. See `lib.rs`.

use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (Some(flag), Some(version)) = (args.next(), args.next()) else {
        return Err("usage: lueur-sign --app-version <version> <file>…".into());
    };
    if flag != "--app-version" || version.is_empty() {
        return Err("usage: lueur-sign --app-version <version> <file>…".into());
    }
    let files: Vec<String> = args.collect();
    if files.is_empty() {
        return Err("no file to sign".into());
    }
    let key_b64 = std::env::var("TAURI_SIGNING_PRIVATE_KEY")
        .map_err(|_| "TAURI_SIGNING_PRIVATE_KEY is not set".to_string())?;
    let password = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
    let key = lueur_sign::secret_key(&key_b64, &password)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    for file in files {
        let path = Path::new(&file);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{file}: no file name"))?;
        let data = std::fs::read(path).map_err(|e| format!("{file}: {e}"))?;
        let sig = lueur_sign::sign_update(&key, &data, name, &version, timestamp)?;
        std::fs::write(format!("{file}.sig"), sig).map_err(|e| format!("{file}.sig: {e}"))?;
        println!("signed {name} for {version}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lueur-sign: {e}");
            ExitCode::FAILURE
        }
    }
}
