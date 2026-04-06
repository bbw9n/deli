use ratatui::{
    Frame,
    layout::Position,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::StatefulImage;
use tui_markdown::from_str as markdown_to_text;

use crate::{
    app::state::{ActiveView, AppState},
    runtime::gnuplot::{ChartRenderMode, plan_chart},
};

pub fn render(frame: &mut Frame<'_>, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(3)])
        .split(frame.area());

    let body = body_layout(chunks[0], state);

    render_sidebar(frame, body[0], state);
    render_main(frame, body[1], state);
    render_details(frame, body[2], state);
    render_footer(frame, chunks[1], state);

    if state.command_palette_open {
        render_palette(frame, centered_rect(frame.area(), 70, 30), state);
    }
}

pub fn config_inspector_inner_area(area: Rect, state: &AppState) -> Rect {
    let body = body_layout(area, state);
    Block::default()
        .title("Inspector")
        .borders(Borders::ALL)
        .inner(body[2])
}

pub fn monitoring_query_inner_area(area: Rect, state: &AppState) -> Rect {
    let body = body_layout(area, state);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body[1]);
    Block::default()
        .title("Metric Query")
        .borders(Borders::ALL)
        .inner(chunks[1])
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

fn render_main(frame: &mut Frame<'_>, area: Rect, state: &mut AppState) {
    match state.active_view {
        ActiveView::Documents => render_documents(frame, area, state),
        ActiveView::Configs => render_configs(frame, area, state),
        ActiveView::Worktree => {
            render_text_panel(frame, area, "Worktree", &state.worktree_lines().join("\n"))
        }
        ActiveView::Monitoring => render_monitoring(frame, area, state),
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
        Span::styled("e", Style::default().fg(Color::Yellow)),
        Span::raw(" edit inspector  "),
        Span::styled("^s", Style::default().fg(Color::Yellow)),
        Span::raw(" save row  "),
        Span::styled("^e", Style::default().fg(Color::Yellow)),
        Span::raw(" export json  "),
        Span::styled("m", Style::default().fg(Color::Yellow)),
        Span::raw(" metric query  "),
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

fn render_monitoring(frame: &mut Frame<'_>, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    let graph_block = Block::default().title("Metric Graph").borders(Borders::ALL);
    let graph_inner = graph_block.inner(chunks[0]);
    frame.render_widget(graph_block, chunks[0]);
    if let Some(image) = state.monitor_image.as_mut() {
        frame.render_stateful_widget(StatefulImage::default(), graph_inner, image);
    } else {
        let graph = Paragraph::new(state.monitor_lines().join("\n")).wrap(Wrap { trim: false });
        frame.render_widget(graph, graph_inner);
    }

    let title = if state.monitor_query_mode {
        "Metric Query [editing]"
    } else {
        "Metric Query [read-only]"
    };
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(chunks[1]);
    frame.render_widget(block, chunks[1]);

    if let Some(editor) = state.monitor_query_editor() {
        frame.render_widget(editor, inner);
        if state.monitor_query_mode
            && let Some((x, y)) = state.monitor_query_cursor(inner)
        {
            frame.set_cursor_position(Position::new(x, y));
        }
    } else {
        let fallback = Paragraph::new(state.monitor_query.clone()).wrap(Wrap { trim: false });
        frame.render_widget(fallback, inner);
    }
}

fn render_text_panel(frame: &mut Frame<'_>, area: Rect, title: &str, content: &str) {
    let paragraph = Paragraph::new(content.to_string())
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_config_inspector(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let title = if state.config_editor_mode {
        if state.config_editor_dirty {
            "Inspector [editing, dirty]"
        } else {
            "Inspector [editing]"
        }
    } else {
        "Inspector [read-only]"
    };
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(editor) = state.config_editor.as_ref() {
        frame.render_widget(editor, inner);
        if state.config_editor_mode
            && let Some((x, y)) = state.config_editor_cursor(inner)
        {
            frame.set_cursor_position(Position::new(x, y));
        }
    } else {
        let fallback = Paragraph::new(state.selected_config_json()).wrap(Wrap { trim: false });
        frame.render_widget(fallback, inner);
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

fn body_layout(area: Rect, state: &AppState) -> Vec<Rect> {
    let detail_width = if state.active_view == ActiveView::Configs {
        48
    } else {
        36
    };
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(50),
            Constraint::Length(detail_width),
        ])
        .split(area)
        .to_vec()
}

pub fn render_snapshot(state: &mut AppState, width: u16, height: u16) -> String {
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
