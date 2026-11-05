//! The app around the window: the shortcuts, the shell's events, the tray,
//! and what closing the window does.
//!
//! GPUI cannot hide a window, so going to the tray closes it and the tray
//! opens a new one; what it shows lives in the [`Lueur`] entity, which
//! outlives it. The app quits only when asked (the tray's Quit, or closing
//! the window when it does not go to the tray).

use crate::i18n::Lang;
use crate::single::Instance;
use crate::state::{self, Lueur, Model, model};
use crate::theme::Theme;
use crate::tray::{self, Pick};
use crate::window::{self, MainWindow};
use async_channel::Receiver;
use gpui::{
    App, AppContext, Bounds, KeyBinding, QuitMode, TitlebarOptions, WindowBounds, WindowOptions,
    actions, px, size,
};
use lueur_shell::tray_model::{self, Outcome};
use lueur_shell::{Event, MINIMIZED_ARG, Shell};
use std::sync::{Arc, Mutex};

actions!(
    lueur,
    [
        GoLighting,
        GoRoom,
        GoBulbs,
        GoSettings,
        ToggleEffect,
        NextControl,
        PreviousControl,
    ]
);

/// The interface face, built in (see `assets/fonts`).
pub const FONT: &str = "Inter";

/// Inter, from the binary: the same face on every system. Loaded once, by
/// `update::launch`, before the splash or the window draws a word.
pub fn load_fonts(cx: &mut App) {
    let fonts = crate::assets::ASSETS
        .iter()
        .filter(|(path, _)| path.starts_with("fonts/") && path.ends_with(".ttf"))
        .map(|(_, bytes)| std::borrow::Cow::Borrowed(*bytes))
        .collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::error!("cannot load Inter: {e}");
    }
}

/// Whether the system asks for less motion. GPUI does not read it, so the
/// app does: on Windows, "Show animations in Windows" off. Read again each
/// time the window opens, so a change made while Lueur sat in the tray
/// counts. Elsewhere it stays off for now.
pub fn follow_reduced_motion(cx: &mut App) {
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
            cx.set_reduce_motion(!animate.as_bool());
        }
    }
    #[cfg(not(windows))]
    let _ = cx;
}

/// Ctrl (⌘ on a Mac) and 1 to 4 switch page, and Enter starts or stops.
#[cfg(target_os = "macos")]
const KEYS: [&str; 5] = ["cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-enter"];
#[cfg(not(target_os = "macos"))]
const KEYS: [&str; 5] = ["ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-enter"];

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(KEYS[0], GoLighting, None),
        KeyBinding::new(KEYS[1], GoRoom, None),
        KeyBinding::new(KEYS[2], GoBulbs, None),
        KeyBinding::new(KEYS[3], GoSettings, None),
        KeyBinding::new(KEYS[4], ToggleEffect, None),
        KeyBinding::new("tab", NextControl, None),
        KeyBinding::new("shift-tab", PreviousControl, None),
    ]);
}

/// Everything the app holds beside the window.
pub struct Started {
    pub shell: Arc<Shell>,
    pub events: Receiver<Event>,
    /// Second launches, turned away by `single`.
    pub second_launches: Receiver<()>,
    pub instance: Instance,
}

/// Set the app up, and open the window unless it starts in the tray.
pub fn run(started: Started, cx: &mut App) {
    let Started {
        shell,
        events,
        second_launches,
        instance,
    } = started;
    cx.set_quit_mode(QuitMode::Explicit);
    bind_keys(cx);
    crate::ui::input::bind_keys(cx);

    let lueur = cx.new(|cx| Lueur::new(Arc::clone(&shell), cx));
    cx.set_global(Model(lueur.clone()));
    let (lang, appearance, behavior) = {
        let l = lueur.read(cx);
        (l.lang(), l.config.interface.appearance, l.config.behavior)
    };
    cx.set_global::<Lang>(lang);
    state::apply_language(lang, &shell, cx);
    cx.set_global(Theme::resolve(appearance, cx.window_appearance()));

    // However Lueur quits, the bulbs are handed back, and the lock is let
    // go so a restart into an update can take over at once.
    let instance = Mutex::new(Some(instance));
    let quitting = Arc::clone(&shell);
    cx.on_app_quit(move |_| {
        quitting.quit();
        if let Some(instance) = instance.lock().ok().and_then(|mut i| i.take()) {
            instance.release();
        }
        async {}
    })
    .detach();

    // The tray, and what it asks for.
    let (picks_tx, picks) = async_channel::unbounded::<Pick>();
    tray::setup(&shell, picks_tx, cx);
    let tray_shell = Arc::clone(&shell);
    cx.spawn(async move |cx| {
        while let Ok(pick) = picks.recv().await {
            cx.update(|cx| match pick {
                Pick::Open => open_main(cx),
                Pick::Item(id) => match tray_model::act(&tray_shell, &id) {
                    Outcome::Done => {}
                    Outcome::Restart => cx.restart(),
                    Outcome::Quit => cx.quit(),
                },
            });
        }
    })
    .detach();

    // The shell's events, on this thread.
    let event_shell = Arc::clone(&shell);
    cx.spawn(async move |cx| {
        while let Ok(event) = events.recv().await {
            cx.update(|cx| handle(event, &event_shell, cx));
        }
    })
    .detach();

    // A second launch brings the window up.
    cx.spawn(async move |cx| {
        while second_launches.recv().await.is_ok() {
            cx.update(open_main);
        }
    })
    .detach();

    // Stay in the tray if the user asked to start there, or if the OS
    // started Lueur at login with `--minimized`. Without a tray to come
    // back from, the window always opens.
    let minimized = std::env::args().any(|a| a == MINIMIZED_ARG);
    if !tray::SUPPORTED || !(behavior.start_minimized || minimized) {
        open_main(cx);
    }
    shell.start();
}

/// An event from the shell, on the app's thread.
fn handle(event: Event, shell: &Arc<Shell>, cx: &mut App) {
    match event {
        Event::TrayStale { always } => tray::update(shell, always, cx),
        Event::FocusMain => open_main(cx),
        event => model(cx).update(cx, |l, cx| l.handle(event, cx)),
    }
}

/// Show the main window: bring it up if it is open, open it otherwise.
pub fn open_main(cx: &mut App) {
    if let Some(window) = cx.windows().into_iter().next() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    follow_reduced_motion(cx);
    let bounds = Bounds::centered(None, size(px(1180.0), px(800.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(960.0), px(640.0))),
        titlebar: Some(TitlebarOptions {
            title: Some(window::title(cx)),
            // On Windows the page draws the bar (see `titlebar.rs`).
            appears_transparent: crate::titlebar::OWN,
            ..Default::default()
        }),
        app_id: Some("com.lueur.app".into()),
        icon: window_icon(),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        window.on_window_should_close(cx, should_close);
        cx.new(|cx| MainWindow::new(window, cx))
    });
    if let Err(e) = opened {
        log::error!("cannot open the window: {e}");
    }
}

/// Restart into the update installed while the app ran, handing the bulbs
/// back first.
pub fn restart_into_update(cx: &mut App) {
    let shell = Arc::clone(&model(cx).read(cx).shell);
    lueur_shell::lights::suspend(&shell, false);
    cx.restart();
}

/// The app's icon, for the window's frame and the taskbar.
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    let png = include_bytes!("../assets/icon-128.png");
    let image = image::load_from_memory(png).ok()?.into_rgba8();
    Some(Arc::new(image))
}

/// Closing goes to the tray when the settings say so, or while a sleep
/// timer runs (quitting would leave the lights on); otherwise it quits.
/// The window closes either way.
fn should_close(_window: &mut gpui::Window, cx: &mut App) -> bool {
    let shell = Arc::clone(&model(cx).read(cx).shell);
    let to_tray = shell
        .lock_config()
        .map(|c| c.behavior.minimize_to_tray)
        .unwrap_or(true);
    let sleeping = lueur_shell::sleep::minutes(&shell).is_some();
    if !(tray::SUPPORTED && (to_tray || sleeping)) {
        cx.quit();
    }
    true
}
