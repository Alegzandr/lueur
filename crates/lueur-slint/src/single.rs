//! One instance at a time: a second launch brings the running one's window
//! up and exits.
//!
//! On Windows this speaks the protocol of Tauri's single-instance plugin
//! (a named mutex, a hidden window that takes the second launch's arguments
//! by `WM_COPYDATA`), under the same names, so the Tauri and the later
//! versions of Lueur recognise each other. Neither ever runs beside the
//! other and fights it for the bulbs, whichever was installed last.
//!
//! The hidden window belongs to the thread that creates it: `acquire` runs
//! on the thread Slint then runs its message loop on, which delivers the
//! message.
//!
//! On Linux it speaks the plugin's D-Bus protocol (the well-known name
//! `com.lueur.app.SingleInstance`), for the same reason. A Mac opens an app
//! bundle once already.

/// The app identifier the Tauri version used (`tauri.conf.json`).
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
const ID: &str = "com.lueur.app";

/// Held for as long as this is the instance that runs.
pub struct Instance {
    #[cfg(windows)]
    imp: Option<windows_impl::Held>,
    /// The session bus connection that owns the name.
    #[cfg(target_os = "linux")]
    imp: Option<zbus::blocking::Connection>,
}

/// Become the instance that runs, calling `on_second_launch` each time
/// another launch is turned away; or, if one already runs, bring it up and
/// return None.
pub fn acquire(on_second_launch: impl Fn() + Send + Sync + 'static) -> Option<Instance> {
    // A development build pointed at a settings folder of its own is a test
    // run: several may run side by side, and beside the installed app.
    if cfg!(debug_assertions) && std::env::var_os("LUEUR_DATA_DIR").is_some() {
        let _ = on_second_launch;
        return Some(Instance {
            #[cfg(any(windows, target_os = "linux"))]
            imp: None,
        });
    }
    #[cfg(windows)]
    {
        windows_impl::acquire(Box::new(on_second_launch)).map(|imp| Instance { imp: Some(imp) })
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::acquire(Box::new(on_second_launch)).map(|imp| Instance { imp })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = on_second_launch;
        Some(Instance {})
    }
}

impl Instance {
    /// Let go before the process ends, so a restart into an update can take
    /// over at once.
    pub fn release(self) {
        #[cfg(windows)]
        if let Some(imp) = self.imp {
            imp.release();
        }
        #[cfg(target_os = "linux")]
        if let Some(connection) = self.imp {
            let _ = connection.release_name(linux_impl::NAME);
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::ID;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
    };
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
        GetWindowThreadProcessId, RegisterClassExW, SMTO_NORMAL, SendMessageTimeoutW, WM_COPYDATA,
        WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
        WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::{HSTRING, PCWSTR};

    /// The plugin's payloads: arguments joined by `|` (older plugins), or
    /// separated by NUL.
    const DATA_LEGACY: usize = 1542;
    const DATA_NUL: usize = 1543;
    const FORWARD_TIMEOUT_MS: u32 = 10_000;

    type Callback = Box<dyn Fn() + Send + Sync>;
    static ON_SECOND_LAUNCH: OnceLock<Callback> = OnceLock::new();

    pub struct Held {
        mutex: HANDLE,
        window: HWND,
    }

    impl Held {
        pub fn release(self) {
            // SAFETY: both handles were created by `acquire` and are released
            // once, here.
            unsafe {
                let _ = ReleaseMutex(self.mutex);
                let _ = CloseHandle(self.mutex);
                let _ = DestroyWindow(self.window);
            }
        }
    }

    pub fn acquire(on_second_launch: Callback) -> Option<Held> {
        let class = HSTRING::from(format!("{ID}-sic"));
        let title = HSTRING::from(format!("{ID}-siw"));
        let mutex_name = HSTRING::from(format!("{ID}-sim"));
        // SAFETY: plain Win32 calls with strings that outlive them.
        unsafe {
            let mutex = CreateMutexW(None, true, &mutex_name).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                forward(&class, &title);
                let _ = CloseHandle(mutex);
                return None;
            }
            let _ = ON_SECOND_LAUNCH.set(on_second_launch);
            let window = listen(&class, &title)?;
            Some(Held { mutex, window })
        }
    }

    /// Hand this launch's arguments to the running instance, which brings
    /// its window up.
    unsafe fn forward(class: &HSTRING, title: &HSTRING) {
        // SAFETY: the caller's strings outlive the calls; `data` outlives the
        // synchronous send.
        unsafe {
            let Ok(hwnd) = FindWindowW(class, title) else {
                return;
            };
            // Windows lets this process bring a window to the front, not the
            // other one: hand that right over before exiting.
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                let _ = AllowSetForegroundWindow(pid);
            }
            let mut data = std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for arg in std::env::args_os() {
                data.push('\0');
                data.push_str(&arg.to_string_lossy());
            }
            let cds = COPYDATASTRUCT {
                dwData: DATA_NUL,
                cbData: data.len() as u32,
                lpData: data.as_ptr() as *mut _,
            };
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&cds as *const _ as isize),
                SMTO_NORMAL,
                FORWARD_TIMEOUT_MS,
                None,
            );
        }
    }

    /// The hidden window a second launch finds and writes to.
    unsafe fn listen(class: &HSTRING, title: &HSTRING) -> Option<HWND> {
        // SAFETY: the class and window are created on this thread with
        // strings that outlive the calls.
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            // The plugin's styles: an invisible popup FindWindowW can find,
            // which a message-only window would not be.
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                class,
                title,
                WS_OVERLAPPED | WS_POPUP | WS_VISIBLE,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_COPYDATA {
            // SAFETY: Windows hands WM_COPYDATA a valid COPYDATASTRUCT for
            // the duration of the call.
            let cds = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            if matches!(cds.dwData, DATA_LEGACY | DATA_NUL) {
                // The arguments say nothing Lueur acts on: any second launch
                // just brings the window up.
                if let Some(callback) = ON_SECOND_LAUNCH.get() {
                    callback();
                }
                return LRESULT(1);
            }
        }
        // SAFETY: the default handling for every other message.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::ID;
    use zbus::blocking::Connection;
    use zbus::blocking::connection::Builder;

    /// The plugin's names: the identifier with `.SingleInstance`, and the
    /// same as a path.
    pub const NAME: &str = "com.lueur.app.SingleInstance";
    const PATH: &str = "/com/lueur/app/SingleInstance";
    const INTERFACE: &str = "org.SingleInstance.DBus";

    type Callback = Box<dyn Fn() + Send + Sync>;

    struct Listener(Callback);

    #[zbus::interface(name = "org.SingleInstance.DBus")]
    impl Listener {
        /// A second launch hands over its arguments and directory. They say
        /// nothing Lueur acts on: any second launch brings the window up.
        fn execute_callback(&self, _argv: Vec<String>, _cwd: String) {
            (self.0)();
        }
    }

    /// Take the name, or hand this launch to the instance that has it and
    /// return None. Without a session bus there is no way to find the
    /// others, so the app starts anyway, as the plugin does.
    pub fn acquire(on_second_launch: Callback) -> Option<Option<Connection>> {
        debug_assert!(NAME.starts_with(ID));
        let builder = match Builder::session() {
            Ok(b) => b,
            Err(e) => {
                log::warn!("single instance: no session bus ({e}); starting anyway");
                return Some(None);
            }
        };
        let built = builder
            .name(NAME)
            .and_then(|b| {
                b.replace_existing_names(false)
                    .allow_name_replacements(false)
                    .serve_at(PATH, Listener(on_second_launch))
            })
            .and_then(|b| b.build());
        match built {
            Ok(connection) => Some(Some(connection)),
            Err(zbus::Error::NameTaken) => {
                let argv: Vec<String> = std::env::args_os()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                let cwd = std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let forwarded = Connection::session().and_then(|c| {
                    c.call_method(
                        Some(NAME),
                        PATH,
                        Some(INTERFACE),
                        "ExecuteCallback",
                        &(argv, cwd),
                    )
                    .map(drop)
                });
                if let Err(e) = forwarded {
                    log::warn!("single instance: cannot reach the running Lueur: {e}");
                }
                None
            }
            Err(e) => {
                log::warn!("single instance: session bus unavailable ({e}); starting anyway");
                Some(None)
            }
        }
    }
}
