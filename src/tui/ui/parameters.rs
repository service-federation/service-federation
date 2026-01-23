use crate::tui::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // Parameters list
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    draw_title(f, chunks[0]);
    draw_parameters(f, app, chunks[1]);
    draw_status_bar(f, chunks[2]);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let text = vec![Line::from(vec![
        Span::styled(
            "Parameters",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (resolved values)", Style::default().fg(Color::DarkGray)),
    ])];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

fn draw_parameters(f: &mut Frame, app: &App, area: Rect) {
    let params = &app.parameters_cache;

    let mut sorted_params: Vec<_> = params.iter().collect();
    sorted_params.sort_by_key(|(k, _)| k.as_str());

    if sorted_params.is_empty() {
        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  No parameters defined",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Parameters are defined in your service-federation.yaml",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![Span::styled(
                "  under the 'parameters' section.",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let block = Block::default()
            .title(" Parameter List ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));

        let paragraph = Paragraph::new(lines).block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Group parameters by prefix (e.g., "api.", "db.", etc.)
    let mut grouped_params: std::collections::HashMap<String, Vec<(&String, &String)>> =
        std::collections::HashMap::new();

    for (key, value) in &sorted_params {
        let prefix = if let Some(pos) = key.find('.') {
            key[..pos].to_string()
        } else {
            "global".to_string()
        };

        grouped_params.entry(prefix).or_default().push((key, value));
    }

    let mut sorted_groups: Vec<_> = grouped_params.iter().collect();
    sorted_groups.sort_by_key(|(k, _)| k.as_str());

    let mut items = vec![];

    for (group, params) in sorted_groups {
        // Group header
        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("  [{}]", group),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])));

        // Parameters in this group
        for (key, value) in params {
            // Determine if value looks like a sensitive parameter
            let is_sensitive = key.to_lowercase().contains("password")
                || key.to_lowercase().contains("secret")
                || key.to_lowercase().contains("token")
                || key.to_lowercase().contains("key");

            let display_value = if is_sensitive && !value.is_empty() {
                "********".to_string()
            } else {
                value.to_string()
            };

            let value_color = if is_sensitive {
                Color::Red
            } else if value.is_empty() {
                Color::DarkGray
            } else {
                Color::Green
            };

            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("{:<30}", key), Style::default().fg(Color::Cyan)),
                Span::styled(" = ", Style::default().fg(Color::DarkGray)),
                Span::styled(display_value, Style::default().fg(value_color)),
            ])));
        }

        items.push(ListItem::new(Line::from("")));
    }

    // Add summary at bottom
    items.push(ListItem::new(Line::from(vec![Span::styled(
        format!("  Total: {} parameters", sorted_params.len()),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )])));

    let block = Block::default()
        .title(" Parameter List (read-only) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect) {
    let shortcuts = vec![
        Span::styled("[Esc]", Style::default().fg(Color::Cyan)),
        Span::raw(" back to dashboard "),
        Span::styled("[q]", Style::default().fg(Color::Cyan)),
        Span::raw("uit "),
        Span::styled("", Style::default().fg(Color::DarkGray)),
        Span::raw("(parameters are read-only)"),
    ];

    let paragraph =
        Paragraph::new(Line::from(shortcuts)).style(Style::default().bg(Color::DarkGray));

    f.render_widget(paragraph, area);
}
