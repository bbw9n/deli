pub mod state;

use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::Picker;

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
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let result = self.run_loop(&mut terminal, &picker);
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        result
    }

    fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        picker: &Picker,
    ) -> Result<()> {
        self.state.refresh_all();
        self.state.sync_monitor_image(picker);
        loop {
            terminal.draw(|frame| render::render(frame, &mut self.state))?;

            match self.event_loop.next()? {
                AppEvent::Tick => self.state.on_tick(),
                AppEvent::Key(key) => {
                    if self.handle_key(key) {
                        break;
                    }
                }
            }
            if self.state.monitor_image.is_none() {
                self.state.sync_monitor_image(picker);
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let code = key.code;
        let modifiers = key.modifiers;

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

        if self.state.config_editor_mode {
            return match code {
                KeyCode::Esc => {
                    self.state.toggle_config_editor_mode();
                    false
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.save_config_editor();
                    false
                }
                KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.export_config_editor();
                    false
                }
                _ => {
                    let area = render::config_inspector_inner_area(
                        ratatui::layout::Rect::new(0, 0, 120, 40),
                        &self.state,
                    );
                    self.state.handle_config_editor_input(key, area);
                    false
                }
            };
        }

        if self.state.monitor_query_mode {
            return match code {
                KeyCode::Esc => {
                    self.state.toggle_monitor_query_mode();
                    false
                }
                KeyCode::Enter => {
                    self.state.apply_monitor_query();
                    false
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.apply_monitor_query();
                    false
                }
                _ => {
                    let area = render::monitoring_query_inner_area(
                        ratatui::layout::Rect::new(0, 0, 120, 40),
                        &self.state,
                    );
                    self.state.handle_monitor_query_input(key, area);
                    false
                }
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
            KeyCode::Char('m') => {
                self.state.toggle_monitor_query_mode();
                false
            }
            KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.save_config_editor();
                false
            }
            KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.export_config_editor();
                false
            }
            KeyCode::Char('e') => {
                self.state.toggle_config_editor_mode();
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
