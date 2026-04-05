use std::time::{Duration, Instant};

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

pub enum AppEvent {
    Tick,
    Key(KeyEvent),
}

pub struct EventLoop {
    tick_rate: Duration,
    last_tick: Instant,
}

impl EventLoop {
    pub fn new(tick_rate: Duration) -> Self {
        Self {
            tick_rate,
            last_tick: Instant::now(),
        }
    }

    pub fn next(&mut self) -> std::io::Result<AppEvent> {
        let timeout = self.tick_rate.saturating_sub(self.last_tick.elapsed());
        if event::poll(timeout)?
            && let CrosstermEvent::Key(key) = event::read()?
        {
            return Ok(AppEvent::Key(key));
        }

        self.last_tick = Instant::now();
        Ok(AppEvent::Tick)
    }
}
