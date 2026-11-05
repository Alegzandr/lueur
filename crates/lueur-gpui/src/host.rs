//! The shell's events, carried from its worker threads to the window's.
//!
//! The shell raises events from any thread (a bulb dropped, the sleep timer
//! ran out, a tray item was picked); GPUI may only be touched from its own.
//! The host queues them, and the app drains the queue on its thread.

use async_channel::{Receiver, Sender};
use lueur_shell::{Event, Host};

pub struct GpuiHost(Sender<Event>);

impl Host for GpuiHost {
    fn emit(&self, event: Event) {
        // Unbounded: only fails once the app is gone, when nobody listens.
        let _ = self.0.try_send(event);
    }
}

/// The host to attach to the shell, and the queue the app drains.
pub fn channel() -> (GpuiHost, Receiver<Event>) {
    let (tx, rx) = async_channel::unbounded();
    (GpuiHost(tx), rx)
}
