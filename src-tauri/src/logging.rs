//! Logs, for whoever needs to see what Lueur did on real hardware.
//!
//! Everything logs through the `log` facade, which most of the tree (Tauri,
//! rustls, the Tuya driver…) already speaks, into the small logger here:
//! each line goes to stderr and to `logs/lueur.log` in the data folder. The
//! file is capped: past [`MAX_FILE`] it becomes `lueur.old.log`, replacing
//! the one before, and a fresh one starts, so the logs never take more than
//! twice that.
//!
//! What is written is set by `LUEUR_LOG`, in `RUST_LOG`'s syntax: `debug`,
//! `lueur_core::device::wire=trace`, `warn,lueur_lib::runner=debug`…
//! Without it, Lueur's own crates log from `info` up and the rest from
//! `warn` up ([`DEFAULT_FILTER`]); targets it names alone are added to that
//! (see [`Filter::from_env`]).
//!
//! No log line may carry a credential (a Hue username, a Nanoleaf token, a
//! Tuya local key, the Smart Life tokens): the device kinds and pairings
//! mask theirs in their `Debug` forms, traced frames go through
//! [`lueur_core::device::wire::Wire`], which masks them, and a test below
//! reads every log call in the sources for a credential's name. Addresses
//! are only logged from `debug` down.

use crate::error::{AppError, AppResult};
use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The variable that sets what is logged.
pub const ENV: &str = "LUEUR_LOG";
/// What is logged when [`ENV`] is not set: `lueur`, `lueur_lib` and
/// `lueur_core` all start with `lueur`.
pub const DEFAULT_FILTER: &str = "warn,lueur=info";
/// The folder in the data folder that holds the logs.
pub const DIR: &str = "logs";
const FILE: &str = "lueur.log";
const OLD_FILE: &str = "lueur.old.log";
/// How large the log file grows before it is set aside.
pub const MAX_FILE: u64 = 2 * 1024 * 1024;

/// Crates whose `trace` output carries what goes over the wire in clear
/// (request lines, headers, bodies), credentials included; capped at
/// `debug` by [`Filter::level_for`].
const CAPPED: &[&str] = &["ureq", "ureq_proto", "rustls", "hyper", "reqwest"];

/// Where the logs are, once [`init`] has run.
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// One `target=level` of a filter; no target is every target.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Directive {
    target: Option<String>,
    level: LevelFilter,
}

/// Which levels each target logs, as `RUST_LOG` writes it: directives
/// separated by commas, each a level (for every target), a target (every
/// level), or `target=level`. A target covers every target it is the start
/// of, and the longest one that matches wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// Longest target first.
    directives: Vec<Directive>,
}

impl Filter {
    /// The filter `spec` describes, and the parts of it that were not
    /// understood (left out). `RUST_LOG`'s `/regex` suffix is not supported.
    pub fn parse(spec: &str) -> (Filter, Vec<String>) {
        let mut directives = Vec::new();
        let mut rejected = Vec::new();
        let (spec, regex) = match spec.split_once('/') {
            Some((spec, regex)) => (spec, Some(regex)),
            None => (spec, None),
        };
        if let Some(regex) = regex {
            rejected.push(format!("/{regex}"));
        }
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let directive = match part.split_once('=') {
                Some((target, level)) => match level.trim().parse::<LevelFilter>() {
                    Ok(level) if !target.trim().is_empty() => Directive {
                        target: Some(target.trim().to_string()),
                        level,
                    },
                    _ => {
                        rejected.push(part.to_string());
                        continue;
                    }
                },
                None => match part.parse::<LevelFilter>() {
                    Ok(level) => Directive {
                        target: None,
                        level,
                    },
                    Err(_) => Directive {
                        target: Some(part.to_string()),
                        level: LevelFilter::Trace,
                    },
                },
            };
            // A later directive for the same target replaces an earlier one.
            directives.retain(|d: &Directive| d.target != directive.target);
            directives.push(directive);
        }
        directives.sort_by_key(|d| std::cmp::Reverse(d.target.as_ref().map_or(0, String::len)));
        (Filter { directives }, rejected)
    }

    /// The filter `LUEUR_LOG` asks for. A spec that sets a level for every
    /// target stands alone; one that only names targets is added to
    /// [`DEFAULT_FILTER`], so `lueur_core::device=trace` leaves everything
    /// else at its usual level.
    pub fn from_env(spec: Option<&str>) -> (Filter, Vec<String>) {
        let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
            return Filter::parse(DEFAULT_FILTER);
        };
        let (own, rejected) = Filter::parse(spec);
        if own.directives.iter().any(|d| d.target.is_none()) {
            return (own, rejected);
        }
        // Later directives win, so the spec's own override the defaults.
        (
            Filter::parse(&format!("{DEFAULT_FILTER},{spec}")).0,
            rejected,
        )
    }

    /// The most detailed level `target` logs. The HTTP and TLS crates in
    /// [`CAPPED`] never go below `debug`, whatever the filter says: at
    /// `trace` they dump the bytes they send, Smart Life's tokens included.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        let level = self
            .directives
            .iter()
            .find(|d| {
                d.target
                    .as_ref()
                    .is_none_or(|t| target.starts_with(t.as_str()))
            })
            .map_or(LevelFilter::Off, |d| d.level);
        let crate_name = target.split("::").next().unwrap_or(target);
        if CAPPED.contains(&crate_name) {
            level.min(LevelFilter::Debug)
        } else {
            level
        }
    }

    /// The most detailed level any target logs.
    pub fn max(&self) -> LevelFilter {
        self.directives
            .iter()
            .map(|d| d.level)
            .max()
            .unwrap_or(LevelFilter::Off)
    }
}

/// The log file, set aside once it reaches its cap.
struct Sink {
    dir: PathBuf,
    file: Option<File>,
    size: u64,
    cap: u64,
    /// While the file cannot be written at all, when to try it again.
    retry_at: Option<Instant>,
}

/// How long a log file that cannot be reopened is left alone.
const REOPEN_AFTER: Duration = Duration::from_secs(60);

impl Sink {
    fn open(dir: &Path, cap: u64) -> io::Result<Sink> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(FILE))?;
        let size = file.metadata()?.len();
        Ok(Sink {
            dir: dir.to_path_buf(),
            file: Some(file),
            size,
            cap,
            retry_at: None,
        })
    }

    fn write(&mut self, line: &[u8]) {
        if self.file.is_none() {
            if self.retry_at.is_some_and(|at| Instant::now() < at) {
                return;
            }
            self.reopen();
        }
        if self.size > 0 && self.size + line.len() as u64 > self.cap {
            self.rotate();
        }
        if let Some(file) = &mut self.file
            && file.write_all(line).is_ok()
        {
            self.size += line.len() as u64;
        }
    }

    /// Set the full file aside, replacing the older one, and start anew.
    /// When it cannot be set aside (another program holding it open, on
    /// Windows), it is emptied instead: the cap holds either way, and the
    /// next attempt waits until the file is full again.
    fn rotate(&mut self) {
        // Closed first: Windows renames an open file only if every handle
        // on it allows it.
        drop(self.file.take());
        let moved = fs::rename(self.dir.join(FILE), self.dir.join(OLD_FILE)).is_ok();
        self.reopen();
        if !moved && self.size > 0 && self.file.is_some() {
            // An append-only handle may not truncate (Windows): another one.
            let emptied = OpenOptions::new()
                .write(true)
                .open(self.dir.join(FILE))
                .and_then(|f| f.set_len(0));
            if emptied.is_ok() {
                self.size = 0;
            } else {
                // Neither moved nor emptied: stop writing it for a while.
                self.file = None;
                self.retry_at = Some(Instant::now() + REOPEN_AFTER);
            }
        }
    }

    /// Open the log file again; on failure, leave it alone for a while.
    fn reopen(&mut self) {
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(FILE))
            .ok();
        self.size = self
            .file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        self.retry_at = self.file.is_none().then(|| Instant::now() + REOPEN_AFTER);
    }
}

struct Logger {
    filter: Filter,
    sink: Option<Mutex<Sink>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = render(
            record,
            &chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        );
        let _ = io::stderr().write_all(line.as_bytes());
        if let Some(sink) = &self.sink
            && let Ok(mut sink) = sink.lock()
        {
            sink.write(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(sink) = &self.sink
            && let Ok(mut sink) = sink.lock()
            && let Some(file) = &mut sink.file
        {
            let _ = file.flush();
        }
    }
}

/// One log line: when, how serious, where from, what, then its fields.
fn render(record: &Record, time: &dyn std::fmt::Display) -> String {
    use std::fmt::Write as _;
    let mut line = format!(
        "{time} {:<5} {}: {}",
        record.level(),
        record.target(),
        record.args()
    );
    struct Fields<'a>(&'a mut String);
    impl<'kvs> log::kv::VisitSource<'kvs> for Fields<'_> {
        fn visit_pair(
            &mut self,
            key: log::kv::Key<'kvs>,
            value: log::kv::Value<'kvs>,
        ) -> Result<(), log::kv::Error> {
            let _ = write!(self.0, " {key}={value}");
            Ok(())
        }
    }
    let _ = record.key_values().visit(&mut Fields(&mut line));
    line.push('\n');
    line
}

/// Start logging, to stderr and to the logs folder in `data_dir`. Called
/// once, first thing; a second call changes nothing.
pub fn init(data_dir: &Path) {
    let (filter, rejected) = Filter::from_env(std::env::var(ENV).ok().as_deref());
    let dir = data_dir.join(DIR);
    let (sink, unopened) = match Sink::open(&dir, MAX_FILE) {
        Ok(sink) => (Some(Mutex::new(sink)), None),
        Err(e) => (None, Some(e)),
    };
    let _ = LOG_DIR.set(dir);
    let max = filter.max();
    if log::set_logger(Box::leak(Box::new(Logger { filter, sink }))).is_err() {
        return;
    }
    log::set_max_level(max);
    for part in rejected {
        log::warn!("{ENV}: ignored {part:?}");
    }
    if let Some(e) = unopened {
        log::warn!("logs go to stderr only: cannot open the log file ({e})");
    }
    log::info!(version = env!("CARGO_PKG_VERSION"); "Lueur starting");
}

/// The logs folder, created if need be.
fn log_dir() -> io::Result<&'static Path> {
    let dir = LOG_DIR
        .get()
        .ok_or_else(|| io::Error::other("logging not started"))?;
    fs::create_dir_all(dir)?;
    Ok(dir)
}

/// Show the logs folder in the file manager.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = log_dir().map_err(|e| AppError::new("logs_unavailable").detail(e))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| {
            log::warn!("cannot open the logs folder: {e}");
            AppError::new("logs_unavailable").detail(e)
        })
}

/// Whether `level` is logged for `target` right now.
#[cfg(test)]
fn logs(filter: &Filter, target: &str, level: log::Level) -> bool {
    level <= filter.level_for(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    #[test]
    fn by_default_lueur_says_more_than_its_dependencies() {
        let (f, rejected) = Filter::parse(DEFAULT_FILTER);
        assert!(rejected.is_empty());
        for ours in ["lueur", "lueur_lib::runner", "lueur_core::device::hue"] {
            assert!(logs(&f, ours, Level::Info), "{ours}");
            assert!(!logs(&f, ours, Level::Debug), "{ours}");
        }
        assert!(logs(&f, "tao::platform", Level::Warn));
        assert!(!logs(&f, "tao::platform", Level::Info));
        assert_eq!(f.max(), LevelFilter::Info);
    }

    #[test]
    fn wire_dumps_of_http_and_tls_crates_never_log() {
        for spec in ["trace", "ureq_proto=trace,rustls=trace", "ureq=trace"] {
            let (f, _) = Filter::from_env(Some(spec));
            for target in [
                "ureq",
                "ureq_proto::client",
                "rustls::conn",
                "hyper",
                "reqwest::connect",
            ] {
                assert!(!logs(&f, target, Level::Trace), "{spec}: {target}");
                // Capped, not silenced.
                if spec == "trace" {
                    assert!(logs(&f, target, Level::Debug), "{target}");
                }
            }
        }
        let (f, _) = Filter::from_env(Some("trace"));
        assert!(logs(&f, "lueur_core::device::wire", Level::Trace));
    }

    #[test]
    fn the_longest_target_wins() {
        let (f, _) = Filter::parse("info,lueur_core=debug,lueur_core::device::wire=trace");
        assert!(logs(&f, "lueur_core::device::wire", Level::Trace));
        assert!(!logs(&f, "lueur_core::device::hue", Level::Trace));
        assert!(logs(&f, "lueur_core::device::hue", Level::Debug));
        assert!(!logs(&f, "zbus", Level::Debug));
        assert_eq!(f.max(), LevelFilter::Trace);
    }

    #[test]
    fn targets_alone_are_added_to_the_defaults() {
        let (f, _) = Filter::from_env(Some("lueur_core::device=trace"));
        assert!(logs(&f, "lueur_core::device::wire", Level::Trace));
        assert!(logs(&f, "lueur_lib::runner", Level::Info));
        assert!(!logs(&f, "lueur_lib::runner", Level::Debug));
        assert!(logs(&f, "tao", Level::Warn) && !logs(&f, "tao", Level::Info));
        // A level for everything stands alone.
        let (f, _) = Filter::from_env(Some("debug"));
        assert!(logs(&f, "lueur_lib::runner", Level::Debug));
        assert!(logs(&f, "tao", Level::Debug));
        let (f, _) = Filter::from_env(Some("lueur=debug"));
        assert!(logs(&f, "lueur_core::device", Level::Debug));
        assert!(!logs(&f, "tao", Level::Info));
        // Unset or blank: the defaults.
        assert_eq!(Filter::from_env(None).0, Filter::parse(DEFAULT_FILTER).0);
        assert_eq!(
            Filter::from_env(Some("  ")).0,
            Filter::parse(DEFAULT_FILTER).0
        );
    }

    #[test]
    fn rust_log_shorthands_are_understood() {
        // A bare target logs everything; a bare level is every target's.
        let (f, _) = Filter::parse("lueur_lib::tuya");
        assert!(logs(&f, "lueur_lib::tuya", Level::Trace));
        assert!(!logs(&f, "lueur_lib::runner", Level::Error));
        let (f, _) = Filter::parse("DEBUG");
        assert!(logs(&f, "anything", Level::Debug));
        let (f, _) = Filter::parse("off");
        assert!(!logs(&f, "lueur", Level::Error));
        // The last word on a target stands.
        let (f, _) = Filter::parse("lueur=trace, lueur=warn");
        assert!(!logs(&f, "lueur", Level::Info));
    }

    #[test]
    fn what_is_not_understood_is_reported_and_left_out() {
        let (f, rejected) = Filter::parse("lueur=loud,=info,warn/foo.*");
        assert_eq!(rejected, ["/foo.*", "lueur=loud", "=info"]);
        assert!(logs(&f, "lueur", Level::Warn));
        assert!(!logs(&f, "lueur", Level::Info));
        // Nothing understood logs nothing.
        assert_eq!(Filter::parse("").0.max(), LevelFilter::Off);
    }

    #[test]
    fn a_line_carries_its_fields() {
        let line = render(
            &Record::builder()
                .level(Level::Warn)
                .target("lueur_lib::runner")
                .args(format_args!("a light dropped"))
                .key_values(&[("family", "wiz"), ("id", "wiz:a8bb")])
                .build(),
            &"2026-10-25 12:00:00.000",
        );
        assert_eq!(
            line,
            "2026-10-25 12:00:00.000 WARN  lueur_lib::runner: a light dropped family=wiz id=wiz:a8bb\n"
        );
    }

    #[test]
    fn the_file_is_set_aside_at_its_cap() {
        let dir = std::env::temp_dir().join(format!("lueur-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut sink = Sink::open(&dir, 100).unwrap();
        let line = [b'x'; 40];
        for _ in 0..2 {
            sink.write(&line);
        }
        assert!(!dir.join(OLD_FILE).exists(), "80 bytes fit");
        sink.write(&line);
        assert_eq!(fs::metadata(dir.join(OLD_FILE)).unwrap().len(), 80);
        assert_eq!(fs::metadata(dir.join(FILE)).unwrap().len(), 40);
        // Again: the older file is replaced, never piled up.
        for _ in 0..3 {
            sink.write(&line);
        }
        assert_eq!(fs::metadata(dir.join(OLD_FILE)).unwrap().len(), 80);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        drop(sink);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_cannot_be_set_aside_is_emptied() {
        let dir = std::env::temp_dir().join(format!("lueur-logs-held-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // A folder in the old file's place: the rename fails, as it does
        // when another program holds the log open on Windows.
        fs::create_dir_all(dir.join(OLD_FILE).join("held")).unwrap();
        let mut sink = Sink::open(&dir, 100).unwrap();
        let line = [b'x'; 40];
        for _ in 0..10 {
            sink.write(&line);
            assert!(fs::metadata(dir.join(FILE)).unwrap().len() <= 100, "capped");
        }
        assert!(sink.file.is_some(), "still written");
        assert_eq!(fs::metadata(dir.join(FILE)).unwrap().len(), sink.size);
        drop(sink);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Every log call in both crates, with the names it formats.
    fn log_calls() -> Vec<(PathBuf, String)> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        walk(&root.join("src"), &mut files);
        walk(&root.join("../crates/lueur-core/src"), &mut files);
        assert!(files.len() > 20, "the sources were found");

        let mut calls = Vec::new();
        for file in files {
            let text = fs::read_to_string(&file).unwrap().replace("\r\n", "\n");
            // Tests are not shipped (and this one quotes log calls).
            let text = text
                .split_once("#[cfg(test)]\nmod tests")
                .map_or(text.as_str(), |(code, _)| code);
            for mac in ["error!(", "warn!(", "info!(", "debug!(", "trace!("] {
                let mut from = 0;
                while let Some(at) = text[from..].find(mac) {
                    let start = from + at + mac.len();
                    from = start;
                    // Not `eprintln!(`, `log_enabled!(` or the like.
                    let before = text[..start - mac.len()].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    calls.push((file.clone(), names_in(&text[start..])));
                }
            }
        }
        calls
    }

    /// The code of a macro call's arguments, up to its closing parenthesis,
    /// with each string literal reduced to the names it interpolates.
    fn names_in(args: &str) -> String {
        let mut out = String::new();
        let mut depth = 1;
        let mut chars = args.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    let mut lit = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                chars.next();
                            }
                            '"' => break,
                            c => lit.push(c),
                        }
                    }
                    // `{name}`, `{name:?}`, `{0}`: keep the names.
                    for inner in lit.split('{').skip(1) {
                        let name: String = inner
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                            .collect();
                        out.push(' ');
                        out.push_str(&name);
                        out.push(' ');
                    }
                }
                '(' => {
                    depth += 1;
                    out.push(c);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                c => out.push(c),
            }
        }
        out
    }

    #[test]
    fn no_log_call_names_a_credential() {
        const CREDENTIALS: [&str; 9] = [
            "username",
            "token",
            "key",
            "local_key",
            "secret",
            "password",
            "access_token",
            "refresh_token",
            "pairings",
        ];
        let calls = log_calls();
        assert!(
            calls.len() > 20,
            "the log calls were found: {}",
            calls.len()
        );
        for (file, code) in calls {
            let words = code
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty());
            for word in words {
                assert!(
                    !CREDENTIALS.contains(&word.to_ascii_lowercase().as_str()),
                    "{}: a log call formats `{word}`: {code}",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn the_scan_sees_what_a_call_formats() {
        let seen = names_in(r#""at {ip}: {e:?}", other) after"#);
        for name in ["ip", "e", "other"] {
            assert!(
                seen.split_whitespace().any(|w| w.trim_matches(',') == name),
                "{seen}"
            );
        }
        assert!(!seen.contains("after"), "{seen}");
        assert!(names_in(r#"id = ep.id.as_str(); "a \"key\" {token}") tail"#).contains("token"));
        assert!(!names_in(r#""no usable key")"#).contains("key"));
    }
}
