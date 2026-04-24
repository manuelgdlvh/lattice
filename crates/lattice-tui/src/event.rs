//! Merged event stream for the main loop.
//!
//! Terminal key events (`crossterm`), queue events (from the agents
//! crate), and log lines from the inspected run all funnel through
//! here so the shell can `select!` on a single receiver.

use crossterm::event::{Event as CtEvent, EventStream};
use futures_util::StreamExt;
use lattice_agents::QueueEvent;
use tokio::sync::mpsc;

/// The unified event type the app loop consumes.
#[derive(Debug)]
pub enum AppEvent {
    Terminal(CtEvent),
    Queue(QueueEvent),
    LogLine(String),
    Tick,
    Shutdown,
}

/// Spawn a background task that forwards `crossterm`'s async event
/// stream into a channel. We do it this way so the `select!` in the
/// shell can own a plain `mpsc::Receiver` instead of fighting with
/// `EventStream`'s lifetimes.
pub fn spawn_terminal_reader() -> mpsc::Receiver<CtEvent> {
    let (tx, rx) = mpsc::channel::<CtEvent>(64);
    tokio::spawn(async move {
        let mut stream = EventStream::new();
        while let Some(Ok(ev)) = stream.next().await {
            if tx.send(ev).await.is_err() {
                break;
            }
        }
    });
    rx
}
