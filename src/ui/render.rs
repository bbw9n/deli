use ratatui::{
    Frame,
    layout::Position,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
};
use ratatui_image::StatefulImage;
use tui_markdown::from_str as markdown_to_text;

use crate::{
    app::state::{ActiveView, AppState},
    runtime::{
        gnuplot::{ChartRenderMode, plan_chart},
        terminal_theme::TerminalColor,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct UiTheme {
    pub panel_bg: Option<Color>,
    pub panel_alt_bg: Option<Color>,
    pub overlay_bg: Option<Color>,
    pub muted_fg: Color,
    pub accent_fg: Color,
}

impl UiTheme {
    pub fn from_terminal_background(background: Option<TerminalColor>) -> Self {
        let Some(background) = background else {
            return Self::default();
        };
        let is_dark = luminance(background) < 128.0;
        let panel_bg = if is_dark {
            nudge(background, 10)
        } else {
            nudge(background, -10)
        };
        let panel_alt_bg = if is_dark {
            nudge(background, 16)
        } else {
            nudge(background, -16)
        };
        let overlay_bg = if is_dark {
            nudge(background, 6)
        } else {
            nudge(background, -6)
        };
        let muted_fg = if is_dark {
            nudge(background, 110)
        } else {
            nudge(background, -110)
        };
        let accent_fg = if is_dark {
            Color::Rgb(224, 198, 140)
        } else {
            Color::Rgb(126, 88, 30)
        };

        Self {
            panel_bg: Some(panel_bg),
            panel_alt_bg: Some(panel_alt_bg),
            overlay_bg: Some(overlay_bg),
            muted_fg,
            accent_fg,
        }
    }
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            panel_bg: None,
            panel_alt_bg: None,
            overlay_bg: None,
            muted_fg: Color::DarkGray,
            accent_fg: Color::Yellow,
        }
    }
}

pub fn render(frame: &mut Frame<'_>, state: &mut AppState) {
    let chunks = shell_layout(frame.area());

    let body = body_layout(chunks[0], state);

    render_sidebar(frame, body[0], state);
    render_main(frame, body[1], state);
    render_details(frame, body[2], state);
    render_messages(frame, chunks[1], state);
    render_footer(frame, chunks[2], state);

    if state.command_palette_open {
        render_palette(frame, centered_rect(frame.area(), 76, 36), state);
    }
}

pub fn config_inspector_inner_area(area: Rect, state: &AppState) -> Rect {
    let shell = shell_layout(area);
    let body = body_layout(shell[0], state);
    Block::default()
        .title("Inspector")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(UiTheme::default().muted_fg))
        .inner(body[2])
}

pub fn document_editor_inner_area(area: Rect, state: &AppState) -> Rect {
    let shell = shell_layout(area);
    let body = body_layout(shell[0], state);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(20)])
        .split(body[1]);
    Block::default()
        .title("Reader")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(UiTheme::default().muted_fg))
        .inner(chunks[1])
}

pub fn monitoring_query_inner_area(area: Rect, state: &AppState) -> Rect {
    let shell = shell_layout(area);
    let body = body_layout(shell[0], state);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body[1]);
    Block::default()
        .title("Metric Query")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(UiTheme::default().muted_fg))
        .inner(chunks[1])
}

pub fn monitoring_graph_inner_area(area: Rect, state: &AppState) -> Rect {
    let shell = shell_layout(area);
    let body = body_layout(shell[0], state);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body[1]);
    Block::default()
        .title("Metric Graph")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(UiTheme::default().muted_fg))
        .inner(chunks[0])
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let items = ActiveView::all()
        .iter()
        .map(|view| {
            let marker = if *view == state.active_view { ">" } else { " " };
            ListItem::new(Line::from(format!("{marker} {}", view.label())))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(panel_block("Views", &state.ui_theme))
        .style(panel_style(&state.ui_theme));
    frame.render_widget(list, area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, state: &mut AppState) {
    match state.active_view {
        ActiveView::Documents => render_documents(frame, area, state),
        ActiveView::Configs => render_configs(frame, area, state),
        ActiveView::Worktree => render_text_panel(
            frame,
            area,
            "Worktree",
            &state.worktree_lines().join("\n"),
            &state.ui_theme,
        ),
        ActiveView::Monitoring => render_monitoring(frame, area, state),
        ActiveView::Services => render_services(frame, area, state),
    }
}

fn render_details(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if state.active_view == ActiveView::Configs {
        render_config_inspector(frame, area, state);
        return;
    }

    let items = state
        .detail_lines()
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(panel_block("Details", &state.ui_theme))
        .style(panel_style(&state.ui_theme));
    frame.render_widget(list, area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let text = Line::from(vec![
        Span::styled("1-5", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" switch view  "),
        Span::styled("j/k", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" move  "),
        Span::styled("^p/^n", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" move alt  "),
        Span::styled(
            "PgUp/PgDn p/n",
            Style::default().fg(state.ui_theme.accent_fg),
        ),
        Span::raw(" scroll doc  "),
        Span::styled("f", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" filter rows  "),
        Span::styled("e", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" edit doc/inspector  "),
        Span::styled("^s", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" save doc/row  "),
        Span::styled("^e", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" export json  "),
        Span::styled("m", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" metric query  "),
        Span::styled("[ ]", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" provider  "),
        Span::styled("{ }", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" preset  "),
        Span::styled("l", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" live poll  "),
        Span::styled("/", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" command palette  "),
        Span::styled("r", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" refresh  "),
        Span::styled("q", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(" quit  "),
        Span::styled(
            format!("workspace: {}", state.workspace_name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let paragraph = Paragraph::new(text)
        .block(panel_block("Status", &state.ui_theme))
        .style(panel_alt_style(&state.ui_theme));
    frame.render_widget(paragraph, area);
}

fn render_messages(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let items = state
        .message_lines()
        .into_iter()
        .map(|line| ListItem::new(Line::from(line)))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(panel_block("*Messages*", &state.ui_theme))
        .style(panel_alt_style(&state.ui_theme));
    frame.render_widget(list, area);
}

fn render_documents(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(20)])
        .split(area);

    let list = List::new(
        state
            .document_list_lines()
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>(),
    )
    .block(panel_block("Documents", &state.ui_theme))
    .style(panel_style(&state.ui_theme));
    frame.render_widget(list, chunks[0]);

    let reader_block = panel_block("Reader", &state.ui_theme);
    let reader_area = reader_block.inner(chunks[1]);
    frame.render_widget(reader_block, chunks[1]);

    if state.document_editor_mode {
        if let Some(editor) = state.document_editor() {
            frame.render_widget(editor, reader_area);
            if let Some((x, y)) = state.document_editor_cursor(reader_area) {
                frame.set_cursor_position(Position::new(x, y));
            }
        } else {
            let reader = Paragraph::new("No document selected")
                .style(panel_style(&state.ui_theme))
                .wrap(Wrap { trim: false });
            frame.render_widget(reader, reader_area);
        }
    } else if let Some(document) = state.current_document() {
        let markdown = markdown_to_text(&document.raw);
        let reader = Paragraph::new(markdown)
            .style(panel_style(&state.ui_theme))
            .scroll((state.document_scroll as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(reader, reader_area);
    } else {
        let reader = Paragraph::new("No document selected")
            .style(panel_style(&state.ui_theme))
            .wrap(Wrap { trim: false });
        frame.render_widget(reader, reader_area);
    }
}

fn render_configs(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let frame_data = state.filtered_config_frame();
    if frame_data.columns.is_empty() {
        render_text_panel(
            frame,
            area,
            "Configs",
            "No config rows loaded",
            &state.ui_theme,
        );
        return;
    }

    let header = Row::new(
        std::iter::once(Cell::from(""))
            .chain(
                frame_data
                    .columns
                    .iter()
                    .map(|column| Cell::from(column.name.clone())),
            )
            .collect::<Vec<_>>(),
    )
    .style(
        Style::default()
            .fg(state.ui_theme.accent_fg)
            .add_modifier(Modifier::BOLD),
    );

    let rows = frame_data
        .rows
        .iter()
        .enumerate()
        .take(40)
        .map(|(index, row)| {
            let marker = if index == state.config_index {
                ">"
            } else {
                " "
            };
            let style = if index == state.config_index {
                Style::default().fg(state.ui_theme.accent_fg)
            } else {
                Style::default()
            };
            Row::new(
                std::iter::once(Cell::from(marker))
                    .chain(row.iter().map(|cell| Cell::from(cell.as_display())))
                    .collect::<Vec<_>>(),
            )
            .style(style)
        });
    let widths = std::iter::once(Constraint::Length(2))
        .chain(frame_data.columns.iter().map(|_| Constraint::Min(12)))
        .collect::<Vec<_>>();
    let filter = if state.config_filter_query.is_empty() {
        "<none>".into()
    } else {
        state.config_filter_query.clone()
    };
    let title = if state.config_filter_mode {
        format!("Configs filter: {filter} [editing]")
    } else {
        format!("Configs filter: {filter}")
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(panel_block(title.as_str(), &state.ui_theme))
        .style(panel_style(&state.ui_theme));
    frame.render_widget(table, area);
}

fn render_services(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(9)])
        .split(area);
    render_service_table(frame, chunks[0], state);
    let actions = List::new(
        state
            .service_action_lines()
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>(),
    )
    .block(panel_block("Operations", &state.ui_theme))
    .style(panel_style(&state.ui_theme));
    frame.render_widget(actions, chunks[1]);
}

fn render_service_table(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let frame_data = state.filtered_service_frame();
    if frame_data.columns.is_empty() {
        render_text_panel(
            frame,
            area,
            "Services",
            "No service resources loaded",
            &state.ui_theme,
        );
        return;
    }

    let header = Row::new(
        std::iter::once(Cell::from(""))
            .chain(
                frame_data
                    .columns
                    .iter()
                    .map(|column| Cell::from(column.name.clone())),
            )
            .collect::<Vec<_>>(),
    )
    .style(
        Style::default()
            .fg(state.ui_theme.accent_fg)
            .add_modifier(Modifier::BOLD),
    );

    let rows = frame_data
        .rows
        .iter()
        .enumerate()
        .take(60)
        .map(|(index, row)| {
            let marker = if index == state.service_index {
                ">"
            } else {
                " "
            };
            let style = if index == state.service_index {
                Style::default().fg(state.ui_theme.accent_fg)
            } else {
                Style::default()
            };
            Row::new(
                std::iter::once(Cell::from(marker))
                    .chain(row.iter().map(|cell| Cell::from(cell.as_display())))
                    .collect::<Vec<_>>(),
            )
            .style(style)
        });
    let widths = std::iter::once(Constraint::Length(2))
        .chain(
            state
                .service_frame
                .columns
                .iter()
                .map(|column| match column.name.as_str() {
                    "name" | "resource" => Constraint::Min(18),
                    "kind" | "type" | "status" | "region" | "namespace" => Constraint::Length(14),
                    _ => Constraint::Min(12),
                }),
        )
        .collect::<Vec<_>>();
    let filter = if state.service_filter_query.is_empty() {
        "<none>".into()
    } else {
        state.service_filter_query.clone()
    };
    let title = if state.service_filter_mode {
        format!("Services filter: {filter} [editing]")
    } else {
        format!("Services filter: {filter}")
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(panel_block(title.as_str(), &state.ui_theme))
        .style(panel_style(&state.ui_theme));
    frame.render_widget(table, area);
}

fn render_monitoring(frame: &mut Frame<'_>, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    let graph_block = panel_block("Metric Graph", &state.ui_theme);
    let graph_inner = graph_block.inner(chunks[0]);
    frame.render_widget(graph_block, chunks[0]);
    if let Some(image) = state.monitor_image.as_mut() {
        frame.render_stateful_widget(StatefulImage::default(), graph_inner, image);
    } else {
        let graph = Paragraph::new(state.monitor_lines().join("\n"))
            .style(panel_style(&state.ui_theme))
            .wrap(Wrap { trim: false });
        frame.render_widget(graph, graph_inner);
    }

    let title = if state.monitor_query_mode {
        "Metric Query [editing]"
    } else {
        "Metric Query [read-only]"
    };
    let block = panel_block(title, &state.ui_theme);
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
        let fallback = Paragraph::new(state.monitor_query.clone())
            .style(panel_style(&state.ui_theme))
            .wrap(Wrap { trim: false });
        frame.render_widget(fallback, inner);
    }
}

fn render_text_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    content: &str,
    theme: &UiTheme,
) {
    let paragraph = Paragraph::new(content.to_string())
        .block(panel_block(title, theme))
        .style(panel_style(theme))
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
    let block = panel_block(title, &state.ui_theme);
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
        let fallback = Paragraph::new(state.selected_config_json())
            .style(panel_style(&state.ui_theme))
            .wrap(Wrap { trim: false });
        frame.render_widget(fallback, inner);
    }
}

fn render_palette(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    frame.render_widget(Clear, area);
    let block = overlay_block("Command", &state.ui_theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);

    let input_block = overlay_block("deli", &state.ui_theme);
    let input_inner = input_block.inner(chunks[0]);
    let input = Line::from(vec![
        Span::styled("› ", Style::default().fg(state.ui_theme.accent_fg)),
        Span::raw(state.command_query.clone()),
    ]);
    frame.render_widget(
        Paragraph::new(input)
            .block(input_block)
            .style(overlay_style(&state.ui_theme)),
        chunks[0],
    );
    let cursor_offset = state.command_cursor.saturating_add(2) as u16;
    let cursor_x = input_inner
        .x
        .saturating_add(cursor_offset)
        .min(input_inner.right().saturating_sub(1));
    frame.set_cursor_position(Position::new(cursor_x, input_inner.y));

    let actions = if state.command_query.starts_with("http://")
        || state.command_query.starts_with("https://")
    {
        vec![
            "Open remote URL",
            "Paste a Notion or Feishu/Lark document link, then press Enter",
        ]
    } else if state.command_query.eq_ignore_ascii_case("connect feishu") {
        vec![
            "Connect Feishu user session",
            "Opens browser auth, captures callback, then stores a local session token",
        ]
    } else {
        vec![
            "open document",
            "open remote url",
            "connect feishu",
            "refresh providers",
            "switch workspace",
            "run worktree command",
        ]
    };
    let list = List::new(
        actions
            .into_iter()
            .map(|action| ListItem::new(Line::from(action)))
            .collect::<Vec<_>>(),
    )
    .block(overlay_block("Suggestions", &state.ui_theme))
    .style(overlay_style(&state.ui_theme));
    frame.render_widget(list, chunks[1]);

    let hint = Paragraph::new("Enter run  Esc cancel  Left/Right move  ^A/^E home/end  ^U/^K kill")
        .style(overlay_style(&state.ui_theme).fg(state.ui_theme.muted_fg));
    frame.render_widget(hint, chunks[2]);
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

fn panel_block<'a>(title: &'a str, theme: &UiTheme) -> Block<'a> {
    Block::default()
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme.accent_fg)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.muted_fg))
        .style(panel_style(theme))
}

fn overlay_block<'a>(title: &'a str, theme: &UiTheme) -> Block<'a> {
    Block::default()
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme.accent_fg)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.muted_fg))
        .style(overlay_style(theme))
}

fn panel_style(theme: &UiTheme) -> Style {
    apply_bg(Style::default(), theme.panel_bg)
}

fn panel_alt_style(theme: &UiTheme) -> Style {
    apply_bg(Style::default(), theme.panel_alt_bg)
}

fn overlay_style(theme: &UiTheme) -> Style {
    apply_bg(Style::default(), theme.overlay_bg)
}

fn apply_bg(style: Style, background: Option<Color>) -> Style {
    match background {
        Some(background) => style.bg(background),
        None => style,
    }
}

fn luminance(color: TerminalColor) -> f32 {
    0.2126 * f32::from(color.r) + 0.7152 * f32::from(color.g) + 0.0722 * f32::from(color.b)
}

fn nudge(color: TerminalColor, amount: i16) -> Color {
    Color::Rgb(
        (i16::from(color.r) + amount).clamp(0, 255) as u8,
        (i16::from(color.g) + amount).clamp(0, 255) as u8,
        (i16::from(color.b) + amount).clamp(0, 255) as u8,
    )
}

fn shell_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(3),
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
