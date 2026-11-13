//! The app around the window: the shell's events, the tray, what closing
//! the window does, and restarting into an update.
//!
//! Closing the window to the tray hides it; the tray, a second launch or
//! the shell bring it back. The app quits only when asked (the tray's Quit,
//! or closing the window when it does not go to the tray).

use crate::single::Instance;
use crate::state::{self, Lueur};
use crate::tray::{self, Pick};
use crate::{AppWindow, Motion, Nav, Tr, i18n, rt, window};
use lueur_shell::tray_model::{self, Outcome};
use lueur_shell::{Event, MINIMIZED_ARG, Shell};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

/// Everything the app holds beside the window.
pub struct Started {
    pub shell: Arc<Shell>,
    pub events: Receiver<Event>,
    /// Second launches, turned away by `single`.
    pub second_launches: Receiver<()>,
    pub instance: Instance,
}

thread_local! {
    /// Let go when the app quits, so a restart into an update can take
    /// over at once.
    static INSTANCE: RefCell<Option<Instance>> = const { RefCell::new(None) };
}

/// Whether the system asks for less motion. Slint does not read it, so the
/// app does: on Windows, "Show animations in Windows" off. Read again each
/// time the window opens, so a change made while Lueur sat in the tray
/// counts. Elsewhere it stays off for now.
pub fn reduced_motion() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        use windows::core::BOOL;
        let mut animate = BOOL(1);
        // SAFETY: the call writes one BOOL into the variable it is handed.
        let read = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut animate as *mut BOOL as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        if read.is_ok() {
            return !animate.as_bool();
        }
    }
    false
}

/// Set the app up, and show the window unless it starts in the tray.
pub fn run(started: Started) {
    let Started {
        shell,
        events,
        second_launches,
        instance,
    } = started;
    INSTANCE.with_borrow_mut(|i| *i = Some(instance));

    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(e) => {
            log::error!("cannot make the window: {e}");
            quit(&shell);
            return;
        }
    };
    i18n::install(&ui.global::<Tr>());
    let lueur = Lueur::new(Arc::clone(&shell));
    let (lang, appearance, behavior) = (
        lueur.lang(),
        lueur.config.interface.appearance,
        lueur.config.behavior,
    );
    rt::install(lueur, &ui, project);
    i18n::set_current(lang, &ui);
    state::apply_language(lang, &shell);
    state::apply_theme(appearance);
    ui.global::<Motion>().set_reduce(reduced_motion());

    window::bind(&ui);
    crate::pages::bind(&ui);
    // The OS switched light or dark: follow it, unless the settings force
    // one.
    ui.global::<Nav>().on_scheme_changed(|| {
        let appearance = rt::read(|l| l.config.interface.appearance);
        state::apply_theme(appearance);
    });

    let close_shell = Arc::clone(&shell);
    ui.window().on_close_requested(move || {
        if should_close(&close_shell) {
            quit(&close_shell);
        }
        rt::with(|l| l.window_shown = false);
        CloseRequestResponse::HideWindow
    });
    #[cfg(windows)]
    minimize_to_tray(&ui, &shell);

    // The tray, and what it asks for.
    let tray_shell = Arc::clone(&shell);
    tray::setup(&shell, move |pick| {
        let shell = Arc::clone(&tray_shell);
        rt::post(move || match pick {
            Pick::Open => open_main(),
            Pick::Item(id) => match tray_model::act(&shell, &id) {
                Outcome::Done => {}
                Outcome::Restart => restart(&shell),
                Outcome::Quit => quit(&shell),
            },
        });
    });

    // The shell's events, on the window's thread.
    let event_shell = Arc::clone(&shell);
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            let shell = Arc::clone(&event_shell);
            rt::post(move || handle(event, &shell));
        }
    });

    // A second launch brings the window up.
    std::thread::spawn(move || {
        while second_launches.recv().is_ok() {
            rt::post(open_main);
        }
    });

    // Stay in the tray if the user asked to start there, or if the OS
    // started Lueur at login with `--minimized`. Without a tray to come
    // back from, the window always opens.
    let minimized = std::env::args().any(|a| a == MINIMIZED_ARG);
    if !tray::SUPPORTED || !(behavior.start_minimized || minimized) {
        open_main();
    }
    shell.start();
}

/// Bring the whole window up to date from the state.
fn project(l: &Lueur, ui: &AppWindow) {
    window::project(l, ui);
    crate::pages::project(l, ui);
}

/// An event from the shell, on the window's thread.
fn handle(event: Event, shell: &Arc<Shell>) {
    match event {
        Event::TrayStale { always } => tray::update(shell, always),
        Event::FocusMain => open_main(),
        event => rt::with(|l| l.handle(event)),
    }
}

/// Show the main window, or bring it to the front.
pub fn open_main() {
    let Some(ui) = rt::ui() else { return };
    ui.global::<Motion>().set_reduce(reduced_motion());
    rt::with(|l| l.window_shown = true);
    if let Err(e) = ui.show() {
        log::error!("cannot show the window: {e}");
    }
    #[cfg(windows)]
    bring_to_front(&ui);
}

/// A window shown again from the tray comes in front of the others, as a
/// second launch expects.
#[cfg(windows)]
fn bring_to_front(ui: &AppWindow) {
    use slint::winit_030::WinitWindowAccessor;
    ui.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

/// When closing quits Lueur, minimising is how it goes to the tray instead
/// of the taskbar; when closing goes to the tray, minimising stays ordinary.
/// Windows only: it is the one system with both a tray and a taskbar (a Mac
/// minimises to its Dock, and Linux has no tray here yet).
#[cfg(windows)]
fn minimize_to_tray(ui: &AppWindow, shell: &Arc<Shell>) {
    use slint::winit_030::winit::event::WindowEvent;
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    let shell = Arc::clone(shell);
    ui.window().on_winit_window_event(move |_, event| {
        // A minimised window is resized to nothing. Whether it is minimised
        // is asked once the event is handled, out of the backend's hands.
        if let WindowEvent::Resized(_) = event {
            let shell = Arc::clone(&shell);
            rt::post(move || {
                let Some(ui) = rt::ui() else { return };
                let minimized = ui
                    .window()
                    .with_winit_window(|w| w.is_minimized())
                    .flatten()
                    .unwrap_or(false);
                if minimized && closing_quits(&shell) {
                    rt::with(|l| {
                        l.arranging = false;
                        l.window_shown = false;
                    });
                    if let Err(e) = ui.hide() {
                        log::error!("cannot hide the window: {e}");
                    }
                }
            });
        }
        EventResult::Propagate
    });
}

/// Closing goes to the tray when the settings say so, or while a sleep
/// timer runs (quitting would leave the lights on); otherwise it quits.
fn should_close(shell: &Arc<Shell>) -> bool {
    // A drag on the plan cut short by the window closing ends here, or the
    // room's changes would wait for its release forever.
    rt::with(|l| l.arranging = false);
    closing_quits(shell)
}

/// Whether closing the window quits Lueur rather than leaving it in the
/// tray.
fn closing_quits(shell: &Arc<Shell>) -> bool {
    let to_tray = shell
        .lock_config()
        .map(|c| c.behavior.minimize_to_tray)
        .unwrap_or(true);
    let sleeping = lueur_shell::sleep::minutes(shell).is_some();
    !(tray::SUPPORTED && (to_tray || sleeping))
}

/// Hand the bulbs back, let the lock go, and end the event loop.
pub fn quit(shell: &Arc<Shell>) {
    shell.quit();
    release_instance();
    tray::remove();
    let _ = slint::quit_event_loop();
}

fn release_instance() {
    if let Some(instance) = INSTANCE.with_borrow_mut(Option::take) {
        instance.release();
    }
}

/// Restart into the update installed while the app ran, handing the bulbs
/// back first.
pub fn restart_into_update() {
    let shell = rt::read(|l| Arc::clone(&l.shell));
    lueur_shell::lights::suspend(&shell, false);
    restart(&shell);
}

/// Start Lueur again and quit this one.
pub fn restart(shell: &Arc<Shell>) {
    shell.quit();
    release_instance();
    relaunch();
    tray::remove();
    let _ = slint::quit_event_loop();
}

/// Start a new process of the app: the executable, or on Linux the
/// AppImage it runs from, where an update was installed.
pub fn relaunch() {
    #[cfg(target_os = "linux")]
    let path = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok());
    #[cfg(not(target_os = "linux"))]
    let path = std::env::current_exe().ok();
    let Some(path) = path else {
        log::error!("cannot find the app to start again");
        return;
    };
    if let Err(e) = std::process::Command::new(&path).spawn() {
        log::error!("cannot start {} again: {e}", path.display());
    }
}
