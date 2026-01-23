use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Shutdown,
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    handler: Option<tokio::task::JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(tick_rate: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tick_rate = Duration::from_millis(tick_rate);

        let handler = tokio::spawn(async move {
            let mut reader = event::EventStream::new();
            let mut tick = tokio::time::interval(tick_rate);

            // Try to set up SIGINT handler, but continue without it if it fails
            let sigint_result =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());
            let mut sigint = match sigint_result {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("Failed to create SIGINT handler: {}. Use 'q' to quit.", e);
                    None
                }
            };

            loop {
                let tick_delay = tick.tick();
                let crossterm_event = reader.next().fuse();

                tokio::select! {
                    _ = async {
                        if let Some(ref mut s) = sigint {
                            s.recv().await
                        } else {
                            std::future::pending::<Option<()>>().await
                        }
                    } => {
                        let _ = tx.send(Event::Shutdown);
                        break;
                    }
                    _ = tick_delay => {
                        let _ = tx.send(Event::Tick);
                    }
                    Some(Ok(evt)) = crossterm_event => {
                        match evt {
                            CrosstermEvent::Key(key) => {
                                let _ = tx.send(Event::Key(key));
                            }
                            CrosstermEvent::Mouse(mouse) => {
                                let _ = tx.send(Event::Mouse(mouse));
                            }
                            CrosstermEvent::Resize(w, h) => {
                                let _ = tx.send(Event::Resize(w, h));
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self {
            rx,
            handler: Some(handler),
        }
    }

    /// Clean up event handler by aborting the task
    pub fn shutdown(mut self) {
        if let Some(handle) = self.handler.take() {
            handle.abort();
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}
