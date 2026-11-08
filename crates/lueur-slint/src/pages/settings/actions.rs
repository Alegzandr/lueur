//! What the Settings page asks of the app: opening at login, the
//! configuration file and the logs folder.

use crate::rt;
use crate::state::Lueur;
use lueur_shell::{logging, service};
use std::path::{Path, PathBuf};

/// Where the save dialog opens: the user's home folder, which every system
/// has.
fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Show `path` in the system's file manager.
pub fn open_with_system(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
}

/// Show a system dialog, then hand what it returned to `done` on the
/// window's thread. A Mac shows its dialogs from the window's thread only,
/// where the dialog runs its own loop meanwhile; elsewhere it waits on a
/// thread of its own.
fn dialog<T: Send + 'static>(
    show: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    if cfg!(target_os = "macos") {
        // After the current callback, so the state is free.
        slint::Timer::single_shot(std::time::Duration::ZERO, move || done(show()));
    } else {
        rt::spawn(show, done);
    }
}

impl Lueur {
    /// Open Lueur at login or stop doing so. The login item is the system's,
    /// so the switch only moves once the system has taken the change.
    pub fn set_run_on_startup(&mut self, on: bool) {
        self.run(
            move |s| service::set_run_on_startup(s, on),
            move |this, ()| this.edit(|c| c.behavior.run_on_startup = on),
        );
    }

    /// Save the configuration, without its credentials, to a file the user
    /// picks, in the system's dialog.
    pub fn export_config(&mut self) {
        dialog(
            || {
                rfd::FileDialog::new()
                    .set_directory(home())
                    .set_file_name(service::EXPORT_FILE_NAME)
                    .save_file()
            },
            |picked| {
                let Some(path) = picked else { return };
                rt::with(|this| {
                    // Edits not saved yet go in the file too: the copy is
                    // written first, then exported.
                    let config = this.config.clone();
                    this.run(
                        move |s| {
                            service::save_config(s, config)?;
                            service::export_config(s, &path)
                        },
                        |this, ()| {
                            let text = this.t("exported");
                            this.inform(text);
                        },
                    );
                });
            },
        );
    }

    /// Replace the configuration with one from a file the user picks. The
    /// shell keeps what must stay this computer's (opening at login, the
    /// pairings).
    pub fn import_config(&mut self) {
        dialog(
            || rfd::FileDialog::new().set_directory(home()).pick_file(),
            |picked| {
                let Some(path) = picked else { return };
                rt::with(|this| {
                    this.run(
                        move |s| service::import_config(s, &path),
                        |this, config| {
                            // Taken as it comes back, so an edit still
                            // waiting to be saved writes the imported
                            // config, not the one it replaced.
                            this.set_config(config);
                            let text = this.t("imported");
                            this.inform(text);
                            this.refresh_status();
                        },
                    );
                });
            },
        );
    }

    /// Show the logs folder in the file manager, to attach to a bug report.
    pub fn open_logs_folder(&mut self) {
        self.run(
            |_| logging::logs_folder().map(|p| p.to_path_buf()),
            |this, path| {
                if let Err(e) = open_with_system(&path) {
                    this.fail(&lueur_shell::error::AppError::internal(e.to_string()));
                }
            },
        );
    }
}
