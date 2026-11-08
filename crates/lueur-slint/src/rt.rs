//! The window's thread: where the app's state lives, how work is sent off
//! it and how its results come back, and when the window is brought up to
//! date.
//!
//! Slint, like every toolkit, may only be touched from the thread that runs
//! its event loop. The app's state ([`Lueur`]) lives there, in a
//! thread-local, and is reached with [`with`]. Commands that block (bulbs,
//! the network, the disk) run on a thread of their own with [`spawn`]; what
//! they return is handed back on the window's thread. Every change to the
//! state asks for one redraw of what the window shows ([`refresh_soon`]),
//! however many changes come in the same turn of the loop.

use crate::AppWindow;
use crate::state::Lueur;
use slint::ComponentHandle;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

type Done = Box<dyn FnOnce(Box<dyn Any + Send>)>;
/// What brings the window up to date from the state.
type Project = fn(&Lueur, &AppWindow);

thread_local! {
    static STATE: RefCell<Option<Lueur>> = const { RefCell::new(None) };
    static UI: RefCell<Option<slint::Weak<AppWindow>>> = const { RefCell::new(None) };
    static JOBS: RefCell<HashMap<u64, Done>> = RefCell::new(HashMap::new());
    static NEXT_JOB: Cell<u64> = const { Cell::new(0) };
    static REFRESH: Cell<bool> = const { Cell::new(false) };
    static PROJECT: RefCell<Option<Project>> = const { RefCell::new(None) };
}

/// Install the state and the window, and what brings the window up to
/// date from the state.
pub fn install(state: Lueur, ui: &AppWindow, project: Project) {
    STATE.with_borrow_mut(|s| *s = Some(state));
    UI.with_borrow_mut(|u| *u = Some(ui.as_weak()));
    PROJECT.with_borrow_mut(|p| *p = Some(project));
    refresh_soon();
}

/// The window, while it exists.
pub fn ui() -> Option<AppWindow> {
    UI.with_borrow(|u| u.as_ref().and_then(|w| w.upgrade()))
}

/// Change the state; the window shows the change once the loop comes
/// round. Must not be called from inside another `with`.
pub fn with<R>(f: impl FnOnce(&mut Lueur) -> R) -> R {
    let r = STATE.with_borrow_mut(|s| f(s.as_mut().expect("the state is installed")));
    refresh_soon();
    r
}

/// Read the state without asking for a redraw.
pub fn read<R>(f: impl FnOnce(&Lueur) -> R) -> R {
    STATE.with_borrow(|s| f(s.as_ref().expect("the state is installed")))
}

/// Whether the state is installed yet (the app may still be behind its
/// splash).
pub fn ready() -> bool {
    STATE.with_borrow(Option::is_some)
}

/// Bring the window up to date once the current turn of the loop is over.
pub fn refresh_soon() {
    if REFRESH.with(|r| r.replace(true)) {
        return;
    }
    slint::Timer::single_shot(Duration::ZERO, refresh_now);
}

/// Bring the window up to date now.
pub fn refresh_now() {
    REFRESH.with(|r| r.set(false));
    let Some(ui) = ui() else { return };
    let Some(project) = PROJECT.with_borrow(|p| *p) else {
        return;
    };
    STATE.with_borrow(|s| {
        if let Some(state) = s.as_ref() {
            project(state, &ui);
        }
    });
}

/// Run `job` on a thread of its own, then `done` with what it returned on
/// the window's thread. `done` need not be `Send`: it waits here.
pub fn spawn<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let id = NEXT_JOB.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    JOBS.with_borrow_mut(|jobs| {
        jobs.insert(
            id,
            Box::new(move |value: Box<dyn Any + Send>| {
                if let Ok(value) = value.downcast::<T>() {
                    done(*value);
                }
            }),
        );
    });
    std::thread::spawn(move || {
        let value: Box<dyn Any + Send> = Box::new(job());
        let delivered = slint::invoke_from_event_loop(move || {
            if let Some(done) = JOBS.with_borrow_mut(|jobs| jobs.remove(&id)) {
                done(value);
            }
        });
        // Only fails once the loop is gone, when nobody waits.
        let _ = delivered;
    });
}

/// Run `f` on the window's thread, from any thread.
pub fn post(f: impl FnOnce() + Send + 'static) {
    let _ = slint::invoke_from_event_loop(f);
}
