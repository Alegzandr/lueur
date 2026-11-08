//! The shell's events, carried from its worker threads to the window's.
//!
//! The shell raises events from any thread (a bulb dropped, the sleep timer
//! ran out, a tray item was picked); Slint may only be touched from its own.
//! The host queues them, and the app hands each to the window's thread.

use lueur_shell::{Event, Host};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel as queue};

pub struct SlintHost(Mutex<Sender<Event>>);

impl Host for SlintHost {
    fn emit(&self, event: Event) {
        // Only fails once the app is gone, when nobody listens.
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(event);
        }
    }
}

/// The host to attach to the shell, and the queue the app reads.
pub fn channel() -> (SlintHost, Receiver<Event>) {
    let (tx, rx) = queue();
    (SlintHost(Mutex::new(tx)), rx)
}
