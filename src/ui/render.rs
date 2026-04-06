use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_code_editor::{editor::Editor, theme};
use tui_markdown::from_str as markdown_to_text;

use crate::{
    app::state::{ActiveView, AppState},
    runtime::gnuplot::{ChartRenderMode, plan_chart},
};

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(3)])
        .split(frame.area());

    let detail_width = if state.active_view == ActiveView::Configs {
        48
    } else {
        36
    };
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(50),
            Constraint::Length(detail_width),
        ])
        .split(chunks[0]);

    render_sidebar(frame, body[0], state);
    render_main(frame, body[1], state);
    render_details(frame, body[2], state);
    render_footer(frame, chunks[1], state);

    if state.command_palette_open {
        render_palette(frame, centered_rect(frame.area(), 70, 30), state);
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let items = ActiveView::all()
        .iter()
        .map(|view| {
            let marker = if *view == state.active_view { ">" } else { " " };
            ListItem::new(Line::from(format!("{marker} {}", view.label())))
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(Block::default().title("Views").borders(Borders::ALL));
    frame.render_widget(list, area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    match state.active_view {
        ActiveView::Documents => render_documents(frame, area, state),
        ActiveView::Configs => render_configs(frame, area, state),
        ActiveView::Worktree => {
            render_text_panel(frame, area, "Worktree", &state.worktree_lines().join("\n"))
        }
        ActiveView::Monitoring => {
            render_text_panel(frame, area, "Monitoring", &state.monitor_lines().join("\n"))
        }
    }
}

fn render_details(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if state.active_view == ActiveView::Configs {
        render_config_inspector(frame, area, state);
        return;
    }

    let lines = state.detail_lines();
    let paragraph = Paragraph::new(lines.join("\n"))
        .block(Block::default().title("Details").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let text = Line::from(vec![
        Span::styled("1-4", Style::default().fg(Color::Yellow)),
        Span::raw(" switch view  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" move  "),
        Span::styled("^p/^n", Style::default().fg(Color::Yellow)),
        Span::raw(" move alt  "),
        Span::styled("PgUp/PgDn p/n", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll doc  "),
        Span::styled("f", Style::default().fg(Color::Yellow)),
        Span::raw(" filter configs  "),
        Span::styled("/", Style::default().fg(Color::Yellow)),
        Span::raw(" command palette  "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(" refresh  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::styled(
            format!("workspace: {}", state.workspace_name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let paragraph =
        Paragraph::new(text).block(Block::default().title("Status").borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}

fn render_documents(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(20)])
        .split(area);

    let list = Paragraph::new(state.document_list_lines().join("\n"))
        .block(Block::default().title("Documents").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(list, chunks[0]);

    let reader_block = Block::default().title("Reader").borders(Borders::ALL);
    let reader_area = reader_block.inner(chunks[1]);
    frame.render_widget(reader_block, chunks[1]);

    if let Some(document) = state.current_document() {
        let markdown = markdown_to_text(&document.raw);
        let reader = Paragraph::new(markdown)
            .scroll((state.document_scroll as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(reader, reader_area);
    } else {
        let reader = Paragraph::new("No document selected").wrap(Wrap { trim: false });
        frame.render_widget(reader, reader_area);
    }
}

fn render_configs(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    render_text_panel(frame, area, "Configs", &state.config_lines().join("\n"));
}

fn render_text_panel(frame: &mut Frame<'_>, area: Rect, title: &str, content: &str) {
    let paragraph = Paragraph::new(content.to_string())
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_config_inspector(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let block = Block::default().title("Inspector").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let payload = state.selected_config_json();
    let editor = Editor::new("json", &payload, theme::vesper())
        .or_else(|_| Editor::new("text", &payload, theme::vesper()));

    match editor {
        Ok(editor) => frame.render_widget(&editor, inner),
        Err(_) => {
            let fallback = Paragraph::new(payload).wrap(Wrap { trim: false });
            frame.render_widget(fallback, inner);
        }
    }
}

fn render_palette(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    frame.render_widget(Clear, area);
    let actions = [
        "open document",
        "refresh providers",
        "switch workspace",
        "run worktree command",
    ];
    let content = actions.join("\n");
    let block = Paragraph::new(content).block(
        Block::default()
            .title(format!("Command Palette: {}", state.command_query))
            .borders(Borders::ALL),
    );
    frame.render_widget(block, area);
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn render_snapshot(state: &AppState, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| render(frame, state)).expect("draw");
    let buffer = terminal.backend().buffer().clone();

    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

pub fn monitor_detail_hint(state: &AppState) -> String {
    match &state.monitor_frame {
        Some(frame) => {
            let plan = plan_chart(frame);
            match plan.mode {
                ChartRenderMode::KittyImage => "Kitty graph renderer ready".into(),
                ChartRenderMode::TextFallback => "Kitty unavailable, using text fallback".into(),
            }
        }
        None => "No chart loaded".into(),
    }
}
