//! What the Settings page asks of the app: opening at login, the
//! configuration file and the logs folder.

use crate::state::Lueur;
use gpui::{Context, PathPromptOptions};
use lueur_shell::error::AppError;
use lueur_shell::{logging, service};
use std::path::PathBuf;

/// Where the save dialog opens: the user's home folder, which every system
/// has.
fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Lueur {
    /// Open Lueur at login or stop doing so. The login item is the system's,
    /// so the switch only moves once the system has taken the change.
    pub fn set_run_on_startup(&mut self, on: bool, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |s| service::set_run_on_startup(s, on),
            move |this, (), cx| this.edit(cx, |c| c.behavior.run_on_startup = on),
        );
    }

    /// Save the configuration, without its credentials, to a file the user
    /// picks.
    pub fn export_config(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_new_path(&home(), Some(service::EXPORT_FILE_NAME));
        cx.spawn(async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| this.fail(&AppError::internal(e), cx));
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                // Edits not saved yet go in the file too: the copy is
                // written first, then exported.
                let config = this.config.clone();
                this.run(
                    cx,
                    move |s| {
                        service::save_config(s, config)?;
                        service::export_config(s, &path)
                    },
                    |this, (), cx| {
                        let text = this.t("exported");
                        this.inform(text, cx);
                    },
                );
            });
        })
        .detach();
    }

    /// Replace the configuration with one from a file the user picks. The
    /// shell keeps what must stay this computer's (opening at login, the
    /// pairings).
    pub fn import_config(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| this.fail(&AppError::internal(e), cx));
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.run(
                    cx,
                    move |s| service::import_config(s, &path),
                    |this, config, cx| {
                        // Taken as it comes back, so an edit still waiting
                        // to be saved writes the imported config, not the
                        // one it replaced.
                        this.set_config(config, cx);
                        let text = this.t("imported");
                        this.inform(text, cx);
                        this.refresh_status(cx);
                    },
                );
            });
        })
        .detach();
    }

    /// Show the logs folder in the file manager, to attach to a bug report.
    pub fn open_logs_folder(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |_| logging::logs_folder().map(|p| p.to_path_buf()),
            |_, path, cx| cx.open_with_system(&path),
        );
    }
}
