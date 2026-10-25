//! One application's sound, for the music show (Windows 10 2004 and later).
//!
//! Windows loops back what a single process tree plays ("process loopback",
//! what OBS's application audio capture uses). The system's audio service
//! does the tapping: Lueur names the process by its id and receives its
//! sound, and never opens it, so this keeps to the anti-cheat rules in
//! `clippy.toml` even pointed at a game. Which applications there are, and
//! which process an executable plays from, come from the audio sessions
//! Windows keeps for whatever has played sound, which name both.

use super::{AudioApp, Tap};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, AudioSessionStateActive,
    DEVICE_STATE_ACTIVE, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, eRender,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, QueryDosDeviceW, VerQueryValueW,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR, PWSTR, Ref, implement};

/// What the loopback asks for. Windows converts whatever the application
/// plays to it.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

/// How long the audio service may take to hand over the capture.
const ACTIVATION: Duration = Duration::from_secs(5);

/// COM for the calling thread, for as long as it lives. A thread that
/// already has it (in another apartment) keeps what it has.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // SAFETY: balanced by the `CoUninitialize` in `drop` when it took.
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread's `CoInitializeEx` succeeded.
            unsafe { CoUninitialize() };
        }
    }
}

/// A string COM allocated, read then freed.
fn take_string(s: PWSTR) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: COM handed over a NUL-terminated string it allocated, freed
    // once read.
    unsafe {
        let text = s.to_string().unwrap_or_default();
        CoTaskMemFree(Some(s.0 as *const _));
        text
    }
}

/// An audio session: who plays (or played) sound, from which executable.
struct Session {
    pid: u32,
    /// The executable's file name, `Spotify.exe`.
    exe: String,
    /// Its path, as the kernel names it (`\Device\HarddiskVolume3\…`).
    path: String,
    /// The name the application gave its sound, often none.
    display: String,
    /// Playing right now, rather than open and quiet.
    active: bool,
}

/// Every application's audio session on every output.
fn sessions() -> windows::core::Result<Vec<Session>> {
    let _com = Com::init();
    let own = std::process::id();
    let mut found = Vec::new();
    // SAFETY: plain COM calls on interfaces this function holds; every
    // string they return is freed by `take_string`.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let Ok(manager) = devices
                .Item(i)
                .and_then(|d| d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None))
            else {
                continue;
            };
            let list = manager.GetSessionEnumerator()?;
            for j in 0..list.GetCount()? {
                let Ok(control) = list
                    .GetSession(j)
                    .and_then(|c| c.cast::<IAudioSessionControl2>())
                else {
                    continue;
                };
                // S_OK for the system sounds, S_FALSE for an application.
                if control.IsSystemSoundsSession() == HRESULT(0) {
                    continue;
                }
                let pid = control.GetProcessId().unwrap_or(0);
                if pid == 0 || pid == own {
                    continue;
                }
                let id = control
                    .GetSessionIdentifier()
                    .map(take_string)
                    .unwrap_or_default();
                let Some(path) = session_path(&id) else {
                    continue;
                };
                let Some(exe) = path.rsplit('\\').next().filter(|e| !e.is_empty()) else {
                    continue;
                };
                found.push(Session {
                    pid,
                    exe: exe.to_string(),
                    path: path.to_string(),
                    display: control
                        .GetDisplayName()
                        .map(take_string)
                        .unwrap_or_default(),
                    active: control
                        .GetState()
                        .is_ok_and(|s| s == AudioSessionStateActive),
                });
            }
        }
    }
    Ok(found)
}

/// The executable's path in a session identifier,
/// `{0.0.0.00000000}.{…}|\Device\HarddiskVolume3\…\Spotify.exe%b{…}`.
fn session_path(id: &str) -> Option<&str> {
    let (_, rest) = id.split_once('|')?;
    let path = rest.split_once("%b").map_or(rest, |(path, _)| path);
    (!path.is_empty()).then_some(path)
}

/// The process `exe` plays from: the one playing now, else any with a
/// session. `None` while it is not open, or has never played anything.
pub fn process_of(exe: &str) -> Option<u32> {
    let sessions = sessions()
        .inspect_err(|e| log::warn!("audio sessions: {e}"))
        .ok()?;
    let mut mine = sessions.iter().filter(|s| s.exe.eq_ignore_ascii_case(exe));
    let first = mine.next()?;
    Some(
        std::iter::once(first)
            .chain(mine)
            .find(|s| s.active)
            .unwrap_or(first)
            .pid,
    )
}

/// The applications that can be listened to: every one with a session,
/// once each, by name.
pub fn list() -> Vec<AudioApp> {
    let sessions = match sessions() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("audio sessions: {e}");
            return Vec::new();
        }
    };
    // Lueur's own session: it plays nothing, but Windows opens one for it.
    let own = std::env::current_exe().ok();
    let own = own
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    let mut apps: Vec<AudioApp> = Vec::new();
    for s in sessions {
        if own.is_some_and(|own| s.exe.eq_ignore_ascii_case(own))
            || apps.iter().any(|a| a.exe.eq_ignore_ascii_case(&s.exe))
        {
            continue;
        }
        apps.push(AudioApp {
            name: app_name(&s),
            exe: s.exe,
        });
    }
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps
}

/// What to call a session's application: the name it gave its sound, else
/// the one its executable describes itself with, else the executable's.
fn app_name(s: &Session) -> String {
    // A name of the form `@%SystemRoot%\…,-123` points into a resource.
    if !s.display.is_empty() && !s.display.starts_with('@') {
        return s.display.clone();
    }
    dos_path(&s.path)
        .and_then(|p| description(&p))
        .unwrap_or_else(|| {
            let stem = s
                .exe
                .rsplit_once('.')
                .map_or(s.exe.as_str(), |(stem, _)| stem);
            let mut chars = stem.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
}

/// A kernel path (`\Device\HarddiskVolume3\…`) as a drive letter's
/// (`C:\…`), for the file APIs.
fn dos_path(path: &str) -> Option<String> {
    let mut target = [0u16; 512];
    for letter in 'A'..='Z' {
        let drive = format!("{letter}:");
        // SAFETY: `target` is a writable buffer whose length the call takes.
        let n =
            unsafe { QueryDosDeviceW(&HSTRING::from(drive.as_str()), Some(&mut target)) } as usize;
        if n == 0 {
            continue;
        }
        // The answer is a list of NUL-terminated strings; the first is it.
        let end = target[..n].iter().position(|&c| c == 0).unwrap_or(n);
        let device = String::from_utf16_lossy(&target[..end]);
        if let Some(rest) = path.strip_prefix(device.as_str())
            && rest.starts_with('\\')
        {
            return Some(format!("{drive}{rest}"));
        }
    }
    None
}

/// The description an executable carries in its version information,
/// `Google Chrome` for `chrome.exe`. Read from the file on disk.
fn description(path: &str) -> Option<String> {
    let path = HSTRING::from(path);
    // SAFETY: the version block is read into a buffer of the size the first
    // call gives, and the pointers `VerQueryValueW` returns point into it,
    // read within the lengths it reports.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, block.as_mut_ptr().cast()).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            block.as_ptr().cast(),
            windows::core::w!("\\VarFileInfo\\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || (len as usize) < 4
        {
            return None;
        }
        let pairs = std::slice::from_raw_parts(ptr as *const u16, len as usize / 2);
        let key = HSTRING::from(format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            pairs[0], pairs[1]
        ));
        if !VerQueryValueW(block.as_ptr().cast(), &key, &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let name = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

/// Told when the audio service has activated the capture.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(mpsc::Sender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

/// An audio client looping back what `pid` and its children play.
fn activate(pid: u32) -> windows::core::Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // Never dropped: a variant's drop frees its blob, and this one's is
    // `params`, on the stack.
    let mut prop = ManuallyDrop::new(PROPVARIANT::default());
    // SAFETY: the variant carries a blob pointing at `params`, which
    // outlives the call that reads it.
    unsafe {
        let inner = &mut *prop.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB {
            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: (&raw mut params).cast(),
        };
        let (tx, rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*prop),
            &handler,
        )?;
        rx.recv_timeout(ACTIVATION)
            .map_err(|_| windows::core::Error::from(E_FAIL))?;
        let mut result = HRESULT(0);
        let mut client = None;
        operation.GetActivateResult(&mut result, &mut client)?;
        result.ok()?;
        client
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?
            .cast()
    }
}

/// A running loopback of one process tree, read on a thread of its own.
pub struct Loopback {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Loopback {
    /// Start listening to `pid`, its sound going to `tap`. `failed` is set
    /// if the capture breaks later.
    pub fn open(pid: u32, tap: Tap, failed: Arc<AtomicBool>) -> windows::core::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("lueur-app-audio".into())
                .spawn(move || {
                    let _com = Com::init();
                    let started = Started::new(pid);
                    let started = match started {
                        Ok(s) => {
                            let _ = ready_tx.send(Ok(()));
                            s
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    if let Err(e) = started.run(&tap, &stop) {
                        log::warn!("application audio: {e}");
                        failed.store(true, Ordering::Relaxed);
                    }
                })
                .map_err(|_| windows::core::Error::from(E_FAIL))?
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Loopback {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(E_FAIL.into())
            }
        }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A loopback client started, with the event it signals when sound is in.
struct Started {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

impl Started {
    fn new(pid: u32) -> windows::core::Result<Self> {
        let client = activate(pid)?;
        let block = CHANNELS * 4;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: CHANNELS,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: COM calls on the client just activated; `format` outlives
        // the call that reads it, and the event handle is closed on drop.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                // 100 ms of room, in 100 ns units.
                1_000_000,
                0,
                &format,
                None,
            )?;
            let event = CreateEventW(None, false, false, PCWSTR::null())?;
            let started = Started {
                capture: client.GetService()?,
                client,
                event,
            };
            started.client.SetEventHandle(started.event)?;
            started.client.Start()?;
            Ok(started)
        }
    }

    /// Hand every packet to `tap` until `stop`.
    fn run(&self, tap: &Tap, stop: &AtomicBool) -> windows::core::Result<()> {
        let channels = usize::from(CHANNELS);
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: the event is this capture's; each buffer is read
            // within the frames it holds, then released.
            unsafe {
                if WaitForSingleObject(self.event, 100) != WAIT_OBJECT_0 {
                    continue;
                }
                while self.capture.GetNextPacketSize()? > 0 {
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    self.capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let n = frames as usize;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        tap.silence(n);
                    } else {
                        tap.feed(std::slice::from_raw_parts(data as *const f32, n * channels));
                    }
                    self.capture.ReleaseBuffer(frames)?;
                }
            }
        }
        Ok(())
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // SAFETY: the client and event are this capture's own.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_executable_in_a_session_identifier() {
        let id = r"{0.0.0.00000000}.{a1b2}|\Device\HarddiskVolume3\Program Files\Spotify\Spotify.exe%b{00000000-0000-0000-0000-000000000000}";
        assert_eq!(
            session_path(id),
            Some(r"\Device\HarddiskVolume3\Program Files\Spotify\Spotify.exe")
        );
        assert_eq!(session_path("{0.0.0.00000000}.{a1b2}|#%b{0}"), Some("#"));
        assert_eq!(session_path("no process"), None);
    }

    /// Lists what plays sound here and listens a moment to the application
    /// `LUEUR_TEST_APP` names (an executable), else the first one:
    /// `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn listens_to_an_application() {
        let apps = list();
        println!("{apps:#?}");
        let wanted = std::env::var("LUEUR_TEST_APP").ok();
        let app = apps
            .iter()
            .find(|a| {
                wanted
                    .as_ref()
                    .is_none_or(|w| a.exe.eq_ignore_ascii_case(w))
            })
            .expect("an application with sound");
        let pid = process_of(&app.exe).expect("its process");
        let tap = Tap::new(SAMPLE_RATE, CHANNELS);
        let failed = Arc::new(AtomicBool::new(false));
        let loopback = Loopback::open(pid, tap.clone(), Arc::clone(&failed)).expect("loopback");
        std::thread::sleep(Duration::from_millis(1500));
        drop(loopback);
        let samples = tap.buffer.lock().unwrap();
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        println!("{} samples from {}, peak {peak}", samples.len(), app.name);
        assert!(!samples.is_empty());
        assert!(!failed.load(Ordering::Relaxed));
    }
}
