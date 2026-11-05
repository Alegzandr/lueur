//! Wayland capture through the xdg-desktop-portal ScreenCast interface.
//!
//! A Wayland compositor lets no client read the screen by itself. `xcap`'s
//! one-shot capture goes through the Screenshot portal, which writes a PNG of
//! the whole desktop to disk on every call, tens of milliseconds a frame, and
//! on some desktops (KDE) asks the user's permission each time. The ambilight
//! instead opens one ScreenCast session, the same portal OBS and browsers use
//! for screen sharing, and reads its PipeWire video stream: the compositor
//! sends a frame when the screen changes, and it is shrunk on arrival to the
//! few hundred pixels the engine reads (see [`scale`]).
//!
//! The user picks the screen in the portal's own dialog, once. Where the
//! portal can remember that choice (version 4 and up), the restore token it
//! hands back is kept in the data folder, and later launches start without a
//! dialog; elsewhere the session is kept open for as long as Lueur runs, so
//! stopping and starting the ambilight never asks twice.
//!
//! Only whole monitors are requested (`types = MONITOR`), never windows.

use super::scale;
use lueur_core::engine::{Frame, PixelFormat};
use pipewire::{
    context::ContextRc,
    core::CoreRc,
    keys::{MEDIA_CATEGORY, MEDIA_ROLE, MEDIA_TYPE},
    loop_::Timeout,
    main_loop::MainLoopRc,
    properties,
    spa::{
        param::{
            ParamType,
            format::{FormatProperties, MediaSubtype, MediaType},
            format_utils,
            video::{VideoFormat, VideoInfoRaw},
        },
        pod::{self, Pod, serialize::PodSerializer},
        utils::{Direction, Fraction, Rectangle, SpaTypes},
    },
    stream::{StreamFlags, StreamListener, StreamRc, StreamState},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DeserializeDict, OwnedFd, OwnedObjectPath, OwnedValue, Type, Value};

const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
/// `SelectSources` source type: whole monitors only.
const SOURCE_MONITOR: u32 = 1;
/// `SelectSources` persist mode: until the user revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;
/// The first ScreenCast version with `persist_mode` and `restore_token`.
const PERSIST_VERSION: u32 = 4;
/// How long the user has to answer the portal's dialog.
const DIALOG_TIMEOUT: Duration = Duration::from_secs(120);
/// How often a wait on the dialog looks whether the capture was stopped.
const CANCEL_POLL: Duration = Duration::from_millis(50);
/// Where the restore token is kept, in the data folder.
const TOKEN_FILE: &str = "screencast-token.json";

/// Whether this is a Wayland session, where the ScreenCast portal is the way
/// to read the screen. The same test `xcap` makes.
pub fn is_wayland() -> bool {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    session == "wayland" || !display.is_empty()
}

/// Why the ScreenCast portal could not be used.
#[derive(Debug)]
pub enum Unavailable {
    /// No portal, or one without ScreenCast: fall back to the one-shot capture.
    NoPortal(String),
    /// The user dismissed the dialog. Falling back would only ask again, a
    /// frame at a time, so the capture stops instead.
    Refused,
    /// The capture was stopped while the dialog was open; it was closed.
    Cancelled,
    /// Anything else (the stream failed to start, the dialog timed out).
    Failed(String),
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unavailable::NoPortal(e) => write!(f, "no ScreenCast portal: {e}"),
            Unavailable::Refused => write!(f, "screen sharing was refused"),
            Unavailable::Cancelled => write!(f, "screen sharing was cancelled"),
            Unavailable::Failed(e) => write!(f, "{e}"),
        }
    }
}

fn failed(what: &'static str) -> impl Fn(zbus::Error) -> Unavailable {
    move |e| Unavailable::Failed(format!("{what}: {e}"))
}

// --- The portal session ---------------------------------------------------

/// An open ScreenCast session. It lives as long as the D-Bus connection that
/// made it, which is kept with it.
struct Session {
    conn: Connection,
    handle: OwnedObjectPath,
    /// The PipeWire node of the shared monitor.
    node: u32,
    /// The monitor index it was opened for: another one asks again.
    monitor: usize,
    /// The portal remembers the choice, so the session may close when the
    /// capture stops (the next one restores it without a dialog).
    persisted: bool,
}

impl Session {
    fn close(self) {
        if let Ok(proxy) = Proxy::new(
            &self.conn,
            PORTAL_SERVICE,
            self.handle.as_str(),
            "org.freedesktop.portal.Session",
        ) {
            let _ = proxy.call_method("Close", &());
        }
        // Ends whatever still listens on it, such as a portal request
        // abandoned while its dialog was open.
        let _ = self.conn.close();
    }
}

/// The session the last capture used, kept while the portal cannot remember
/// the user's choice by itself.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// The restore token, as stored on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredToken {
    monitor: usize,
    token: String,
}

fn token_path() -> PathBuf {
    crate::paths::resolve_data_dir().join(TOKEN_FILE)
}

fn load_token(monitor: usize) -> Option<String> {
    let text = std::fs::read_to_string(token_path()).ok()?;
    let stored: StoredToken = serde_json::from_str(&text).ok()?;
    (stored.monitor == monitor && !stored.token.is_empty()).then_some(stored.token)
}

fn save_token(monitor: usize, token: &str) {
    let path = token_path();
    let stored = StoredToken {
        monitor,
        token: token.to_string(),
    };
    let saved = serde_json::to_string(&stored)
        .map_err(|e| e.to_string())
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, text).map_err(|e| e.to_string())
        });
    if let Err(e) = saved {
        log::warn!("could not keep the screen sharing permission: {e}");
    }
}

/// A fresh token for a portal request or session, unique in this process.
fn token() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "lueur{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
struct CreateSessionResponse {
    session_handle: Option<String>,
}

#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
struct StreamProperties {
    size: Option<(i32, i32)>,
}

#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
struct StartResponse {
    streams: Option<Vec<(u32, StreamProperties)>>,
    restore_token: Option<String>,
}

/// Call a portal method that answers through a `Request` object, and wait
/// for that answer.
///
/// The `Response` signal is subscribed to before the call, on a helper
/// thread that hands the answer over: the user may take a while over the
/// dialog, and the wait has to give up when the capture is stopped (`cancel`)
/// or after [`DIALOG_TIMEOUT`]. Giving up closes the dialog; the helper
/// thread ends with the connection, which the caller then closes.
fn request<B, T>(
    conn: &Connection,
    portal: &Proxy<'_>,
    method: &'static str,
    handle_token: &str,
    body: &B,
    cancel: &AtomicBool,
) -> Result<T, Unavailable>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    T: for<'de> serde::Deserialize<'de> + Type + Send + 'static,
{
    let sender = conn
        .unique_name()
        .ok_or_else(|| Unavailable::Failed("no D-Bus name".into()))?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{PORTAL_PATH}/request/{sender}/{handle_token}");
    let answers = Proxy::new(conn, PORTAL_SERVICE, path, "org.freedesktop.portal.Request")
        .map_err(failed("portal request"))?;
    let dialog = answers.clone();

    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let (answer_tx, answer_rx) = mpsc::channel::<Result<(u32, T), String>>();
    std::thread::Builder::new()
        .name("lueur-portal".into())
        .spawn(move || {
            let mut signals = match answers.receive_signal("Response") {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let answer = match signals.next() {
                Some(message) => {
                    let body = message.body();
                    body.deserialize::<(u32, T)>().map_err(|e| e.to_string())
                }
                None => Err("no answer from the portal".to_string()),
            };
            let _ = answer_tx.send(answer);
        })
        .map_err(|e| Unavailable::Failed(e.to_string()))?;
    ready_rx
        .recv()
        .map_err(|e| Unavailable::Failed(e.to_string()))?
        .map_err(Unavailable::Failed)?;

    portal
        .call_method(method, body)
        .map_err(|e| Unavailable::Failed(format!("{method}: {e}")))?;
    let deadline = Instant::now() + DIALOG_TIMEOUT;
    let gave_up = loop {
        if cancel.load(Ordering::SeqCst) {
            break Unavailable::Cancelled;
        }
        match answer_rx.recv_timeout(CANCEL_POLL) {
            Ok(Ok((0, answer))) => return Ok(answer),
            Ok(Ok((1, _))) => return Err(Unavailable::Refused),
            Ok(Ok((code, _))) => {
                return Err(Unavailable::Failed(format!("{method} ended with {code}")));
            }
            Ok(Err(e)) => return Err(Unavailable::Failed(format!("{method}: {e}"))),
            Err(RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(_) => break Unavailable::Failed(format!("{method}: no answer in time")),
        }
    };
    // Takes the dialog off the screen.
    let _ = dialog.call_method("Close", &());
    Err(gave_up)
}

/// Open a new ScreenCast session for one monitor, restoring the user's last
/// choice where the portal allows it.
fn open_session(monitor: usize, cancel: &AtomicBool) -> Result<Session, Unavailable> {
    let conn = Connection::session().map_err(|e| Unavailable::NoPortal(e.to_string()))?;
    let portal = Proxy::new(&conn, PORTAL_SERVICE, PORTAL_PATH, SCREENCAST)
        .map_err(|e| Unavailable::NoPortal(e.to_string()))?;
    // Reading the version is what tells a missing portal apart.
    let version: u32 = portal
        .get_property("version")
        .map_err(|e| Unavailable::NoPortal(e.to_string()))?;

    let handle_token = token();
    let session_token = token();
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("session_handle_token", Value::from(session_token.as_str()));
    let created: CreateSessionResponse = match request(
        &conn,
        &portal,
        "CreateSession",
        &handle_token,
        &options,
        cancel,
    ) {
        Ok(created) => created,
        Err(e) => {
            let _ = conn.close();
            return Err(e);
        }
    };
    let handle = created
        .session_handle
        .ok_or_else(|| Unavailable::Failed("no session handle".into()))?;
    let handle = OwnedObjectPath::try_from(handle)
        .map_err(|e| Unavailable::Failed(format!("session handle: {e}")))?;

    let persist = version >= PERSIST_VERSION;
    let restore = if persist { load_token(monitor) } else { None };
    let handle_token = token();
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("types", Value::from(SOURCE_MONITOR));
    options.insert("multiple", Value::from(false));
    if persist {
        options.insert("persist_mode", Value::from(PERSIST_UNTIL_REVOKED));
        if let Some(t) = restore.as_deref() {
            options.insert("restore_token", Value::from(t));
        }
    }
    let selected: Result<HashMap<String, OwnedValue>, _> = request(
        &conn,
        &portal,
        "SelectSources",
        &handle_token,
        &(&handle, options),
        cancel,
    );
    let opened = selected.and_then(|_| {
        let handle_token = token();
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(handle_token.as_str()));
        request::<_, StartResponse>(
            &conn,
            &portal,
            "Start",
            &handle_token,
            &(&handle, "", options),
            cancel,
        )
    });
    let started = match opened {
        Ok(started) => started,
        Err(e) => {
            Session {
                conn,
                handle,
                node: 0,
                monitor,
                persisted: false,
            }
            .close();
            return Err(e);
        }
    };

    // Each token is good for one restore; the portal hands back the next.
    let persisted = match started.restore_token.as_deref() {
        Some(t) if persist && !t.is_empty() => {
            save_token(monitor, t);
            true
        }
        _ => false,
    };
    let (node, size) = started
        .streams
        .and_then(|s| s.into_iter().next())
        .map(|(node, props)| (node, props.size))
        .ok_or_else(|| Unavailable::Failed("the portal shared no screen".into()))?;
    if let Some((w, h)) = size {
        log::info!("sharing a {w}x{h} screen through the ScreenCast portal");
    }
    Ok(Session {
        conn,
        handle,
        node,
        monitor,
        persisted,
    })
}

/// A file descriptor onto the PipeWire remote that carries `session`'s stream.
fn open_remote(session: &Session) -> Result<std::os::fd::OwnedFd, Unavailable> {
    let portal = Proxy::new(&session.conn, PORTAL_SERVICE, PORTAL_PATH, SCREENCAST)
        .map_err(failed("portal"))?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let fd: OwnedFd = portal
        .call("OpenPipeWireRemote", &(&session.handle, options))
        .map_err(failed("PipeWire remote"))?;
    Ok(fd.into())
}

/// The session for `monitor`: the one kept from an earlier capture when it is
/// still open, else a new one.
fn session_remote(
    monitor: usize,
    cancel: &AtomicBool,
) -> Result<(u32, std::os::fd::OwnedFd), Unavailable> {
    let mut kept = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(session) = kept.take() {
        if session.monitor == monitor
            && let Ok(fd) = open_remote(&session)
        {
            let node = session.node;
            *kept = Some(session);
            return Ok((node, fd));
        }
        session.close();
    }
    let session = open_session(monitor, cancel)?;
    let fd = open_remote(&session)?;
    let node = session.node;
    *kept = Some(session);
    Ok((node, fd))
}

/// The capture stopped: a session the portal can restore is closed, which
/// also takes down the desktop's "screen is shared" indicator; any other is
/// kept for the next start.
fn release_session() {
    let mut kept = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    if kept.as_ref().is_some_and(|s| s.persisted)
        && let Some(session) = kept.take()
    {
        session.close();
    }
}

// --- The PipeWire stream --------------------------------------------------

/// What the stream callbacks share with [`ScreenCastSource::grab`]. Both run
/// on the capture thread, inside `Loop::iterate`.
#[derive(Default)]
struct Shared {
    format: VideoInfoRaw,
    /// The newest shrunk frame, and whether `grab` has not taken it yet.
    frame: Option<Frame>,
    fresh: bool,
    error: Option<String>,
}

/// The byte order of a negotiated video format, if it is one the engine reads.
fn pixel_format(format: VideoFormat) -> Option<PixelFormat> {
    if format == VideoFormat::BGRx || format == VideoFormat::BGRA {
        Some(PixelFormat::Bgra)
    } else if format == VideoFormat::RGBx || format == VideoFormat::RGBA {
        Some(PixelFormat::Rgba)
    } else {
        None
    }
}

/// A monitor shared through the ScreenCast portal. Not `Send`: the PipeWire
/// loop is iterated by the thread that built it, inside `grab`.
pub struct ScreenCastSource {
    // Dropped first to last: the listener before its stream, the stream
    // before the connection and the loop it runs on.
    _listener: StreamListener<()>,
    _stream: StreamRc,
    _core: CoreRc,
    _context: ContextRc,
    main_loop: MainLoopRc,
    shared: Rc<RefCell<Shared>>,
    frame: Option<Frame>,
}

impl ScreenCastSource {
    /// Share the monitor the user picks (or picked before) for `monitor`.
    /// Setting `cancel` closes a dialog still open and returns at once.
    pub fn open(monitor: usize, cancel: &AtomicBool) -> Result<Self, Unavailable> {
        if cancel.load(Ordering::SeqCst) {
            return Err(Unavailable::Cancelled);
        }
        let (node, fd) = session_remote(monitor, cancel)?;
        Self::connect(node, fd).inspect_err(|_| release_session())
    }

    fn connect(node: u32, fd: std::os::fd::OwnedFd) -> Result<Self, Unavailable> {
        let pw = |what: &'static str| {
            move |e: pipewire::Error| Unavailable::Failed(format!("{what}: {e}"))
        };
        let main_loop = MainLoopRc::new(None).map_err(pw("PipeWire loop"))?;
        let context = ContextRc::new(&main_loop, None).map_err(pw("PipeWire context"))?;
        let core = context
            .connect_fd_rc(fd, None)
            .map_err(pw("PipeWire connection"))?;
        let stream = StreamRc::new(
            core.clone(),
            "Lueur",
            properties::properties! {
                *MEDIA_TYPE => "Video",
                *MEDIA_CATEGORY => "Capture",
                *MEDIA_ROLE => "Screen",
            },
        )
        .map_err(pw("PipeWire stream"))?;

        let shared = Rc::new(RefCell::new(Shared::default()));
        let on_param = Rc::clone(&shared);
        let on_state = Rc::clone(&shared);
        let on_frame = Rc::clone(&shared);
        let listener = stream
            .add_local_listener_with_user_data(())
            .state_changed(move |_, _, old, state| match state {
                StreamState::Error(e) => {
                    on_state.borrow_mut().error = Some(format!("screen stream: {e}"));
                }
                // Sharing stopped from the desktop (its "stop sharing"
                // button): the stream does not come back, and the last image
                // must not pass for the screen.
                StreamState::Unconnected if !matches!(old, StreamState::Unconnected) => {
                    on_state.borrow_mut().error = Some("screen sharing was stopped".into());
                }
                _ => {}
            })
            .param_changed(move |_, _, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != ParamType::Format.as_raw() {
                    return;
                }
                let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                    return;
                };
                if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                    return;
                }
                let mut shared = on_param.borrow_mut();
                if shared.format.parse(param).is_err() {
                    shared.error = Some("unreadable screen format".into());
                }
            })
            .process(move |stream, _| {
                // Only the newest buffer matters; older ones go straight back.
                let mut newest = None;
                while let Some(buffer) = stream.dequeue_buffer() {
                    newest = Some(buffer);
                }
                let Some(mut buffer) = newest else {
                    return;
                };
                let mut shared = on_frame.borrow_mut();
                let info = shared.format;
                let Some(format) = pixel_format(info.format()) else {
                    return;
                };
                let size = info.size();
                let Some(data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let chunk = data.chunk();
                let (offset, used, stride) = (
                    chunk.offset() as usize,
                    chunk.size() as usize,
                    chunk.stride(),
                );
                // An empty chunk is a frame where only the pointer moved.
                if used == 0 {
                    return;
                }
                let stride = if stride > 0 {
                    stride as usize
                } else {
                    size.width as usize * 4
                };
                let Some(bytes) = data.data() else {
                    return;
                };
                let end = offset.saturating_add(used).min(bytes.len());
                let Some(pixels) = bytes.get(offset..end) else {
                    return;
                };
                let out = shared
                    .frame
                    .get_or_insert_with(|| Frame::new(0, 0, format, Vec::new()));
                // A frame that does not fit what was negotiated is skipped;
                // the next one will be fine.
                if scale::shrink_into(pixels, size.width, size.height, stride, format, out).is_ok()
                {
                    shared.fresh = true;
                }
            })
            .register()
            .map_err(pw("PipeWire listener"))?;

        // Any size and rate the compositor offers, in a 4-byte layout the
        // engine reads without conversion.
        let format = pod::object!(
            SpaTypes::ObjectParamFormat,
            ParamType::EnumFormat,
            pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
            pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
            pod::property!(
                FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                VideoFormat::BGRx,
                VideoFormat::BGRx,
                VideoFormat::BGRA,
                VideoFormat::RGBx,
                VideoFormat::RGBA,
            ),
            pod::property!(
                FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                Rectangle {
                    width: 1920,
                    height: 1080
                },
                Rectangle {
                    width: 1,
                    height: 1
                },
                Rectangle {
                    width: 8192,
                    height: 8192
                }
            ),
            pod::property!(
                FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                Fraction { num: 60, denom: 1 },
                Fraction { num: 0, denom: 1 },
                Fraction {
                    num: 1000,
                    denom: 1
                }
            ),
        );
        let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &pod::Value::Object(format))
            .map_err(|e| Unavailable::Failed(format!("stream format: {e:?}")))?
            .0
            .into_inner();
        let param =
            Pod::from_bytes(&bytes).ok_or_else(|| Unavailable::Failed("stream format".into()))?;
        stream
            .connect(
                Direction::Input,
                Some(node),
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut [param],
            )
            .map_err(pw("PipeWire stream connection"))?;

        Ok(Self {
            _listener: listener,
            _stream: stream,
            _core: core,
            _context: context,
            main_loop,
            shared,
            frame: None,
        })
    }

    /// Return the newest screen image, waiting up to `timeout` for the
    /// compositor to send one. A still screen sends nothing, and the previous
    /// image, still current, is returned again.
    pub fn grab(&mut self, timeout: Duration) -> Result<&Frame, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            self.main_loop.loop_().iterate(Timeout::Finite(left));
            let mut shared = self.shared.borrow_mut();
            if let Some(e) = &shared.error {
                return Err(e.clone());
            }
            if shared.fresh {
                shared.fresh = false;
                // Swapped rather than copied: the callback refills the old one.
                std::mem::swap(&mut self.frame, &mut shared.frame);
                break;
            }
            if left.is_zero() {
                break;
            }
        }
        self.frame
            .as_ref()
            .ok_or_else(|| "no screen image yet".to_string())
    }
}

impl Drop for ScreenCastSource {
    fn drop(&mut self) {
        release_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_four_byte_layouts() {
        assert_eq!(pixel_format(VideoFormat::BGRx), Some(PixelFormat::Bgra));
        assert_eq!(pixel_format(VideoFormat::BGRA), Some(PixelFormat::Bgra));
        assert_eq!(pixel_format(VideoFormat::RGBx), Some(PixelFormat::Rgba));
        assert_eq!(pixel_format(VideoFormat::RGBA), Some(PixelFormat::Rgba));
        assert_eq!(pixel_format(VideoFormat::RGB), None);
    }

    #[test]
    fn request_tokens_are_unique_object_path_elements() {
        let (a, b) = (token(), token());
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
