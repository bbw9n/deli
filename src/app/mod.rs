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
        if self.state.command_palette_open {
            return match code {
                KeyCode::Esc => {
                    self.state.command_palette_open = false;
                    false
                }
                KeyCode::Backspace => {
                    self.state.command_query.pop();
                    false
                }
                KeyCode::Char(c) => {
                    self.state.command_query.push(c);
                    false
                }
                _ => false,
            };
        }

        if self.state.config_filter_mode {
            return match code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.state.clear_filter_mode();
                    false
                }
                KeyCode::Backspace => {
                    self.state.pop_filter_char();
                    false
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.append_filter_char(c);
                    false
                }
                _ => false,
            };
        }

        match code {
            KeyCode::Char('q') => true,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => true,
            KeyCode::Char('1') => {
                self.state.activate_view(ActiveView::Documents);
                false
            }
            KeyCode::Char('2') => {
                self.state.activate_view(ActiveView::Configs);
                false
            }
            KeyCode::Char('3') => {
                self.state.activate_view(ActiveView::Worktree);
                false
            }
            KeyCode::Char('4') => {
                self.state.activate_view(ActiveView::Monitoring);
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.state.move_selection(1);
                false
            }
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.move_selection(1);
                false
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_selection(-1);
                false
            }
            KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.move_selection(-1);
                false
            }
            KeyCode::PageDown => {
                self.state.scroll_current(8);
                false
            }
            KeyCode::Char('n') => {
                self.state.scroll_current(8);
                false
            }
            KeyCode::PageUp => {
                self.state.scroll_current(-8);
                false
            }
            KeyCode::Char('p') => {
                self.state.scroll_current(-8);
                false
            }
            KeyCode::Char('r') => {
                self.state.refresh_all();
                false
            }
            KeyCode::Char('f') => {
                self.state.toggle_config_filter_mode();
                false
            }
            KeyCode::Char('/') => {
                self.state.command_palette_open = !self.state.command_palette_open;
                false
            }
            KeyCode::Esc => {
                self.state.clear_filter_mode();
                self.state.command_palette_open = false;
                false
            }
            _ => false,
        }
    }
}
