//! Keeps the computer from going to sleep on its own while the sleep timer
//! runs: asleep, it could not put the lights out. Only the idle sleep is held
//! off. The screen still turns off, and the person can still shut the
//! computer down or put it to sleep.
//!
//! On Linux nothing is held yet: the desktops each have their own way.

/// Held for as long as the computer must stay awake. On Windows the request
/// belongs to the thread that made it, so it is dropped on that thread.
pub struct Awake(imp::Guard);

impl Awake {
    pub fn hold() -> Self {
        Self(imp::Guard::hold())
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    pub struct Guard;

    impl Guard {
        pub fn hold() -> Self {
            // SAFETY: only sets this thread's power request.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            Guard
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: as above; this clears the request.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command};

    /// `caffeinate`, which holds the request for as long as it runs. Told
    /// to watch Lueur, it ends with it, even if Lueur crashes.
    pub struct Guard(Option<Child>);

    impl Guard {
        pub fn hold() -> Self {
            let pid = std::process::id().to_string();
            let child = Command::new("/usr/bin/caffeinate")
                .args(["-i", "-w", &pid])
                .spawn();
            if let Err(e) = &child {
                log::warn!("sleep timer: cannot keep the computer awake: {e}");
            }
            Guard(child.ok())
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub struct Guard;

    impl Guard {
        pub fn hold() -> Self {
            Guard
        }
    }
}
