pub mod state;

use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    app::state::{ActiveView, AppState},
    models::config::AppConfig,
    providers::registry::ProviderRegistry,
    runtime::event::{AppEvent, EventLoop},
    ui::render,
};

pub struct App {
    state: AppState,
    event_loop: EventLoop,
}

impl App {
    pub fn new(config: AppConfig, workspace_override: Option<String>) -> Result<Self> {
        let registry = ProviderRegistry::from_config(&config);
        let state = AppState::new(config, registry, workspace_override)?;
        Ok(Self {
            state,
            event_loop: EventLoop::new(Duration::from_secs(1)),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self.run_loop(&mut terminal);
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        self.state.refresh_all();
        loop {
            terminal.draw(|frame| render::render(frame, &self.state))?;

            match self.event_loop.next()? {
                AppEvent::Tick => self.state.on_tick(),
                AppEvent::Key(key) => {
                    if self.handle_key(key.code, key.modifiers) {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        match code {
            KeyCode::Char('q') => true,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => true,
            KeyCode::Char('1') => {
                self.state.active_view = ActiveView::Documents;
                false
            }
            KeyCode::Char('2') => {
                self.state.active_view = ActiveView::Configs;
                false
            }
            KeyCode::Char('3') => {
                self.state.active_view = ActiveView::Worktree;
                false
            }
            KeyCode::Char('4') => {
                self.state.active_view = ActiveView::Monitoring;
                false
            }
            KeyCode::Char('r') => {
                self.state.refresh_all();
                false
            }
            KeyCode::Char('/') => {
                self.state.command_palette_open = !self.state.command_palette_open;
                false
            }
            KeyCode::Esc => {
                self.state.command_palette_open = false;
                false
            }
            KeyCode::Char(c) if self.state.command_palette_open => {
                self.state.command_query.push(c);
                false
            }
            KeyCode::Backspace if self.state.command_palette_open => {
                self.state.command_query.pop();
                false
            }
            _ => false,
        }
    }
}
