use crate::tui::app::{App, View};
use ratatui::Frame;

pub mod dashboard;
pub mod details;
pub mod graph;
pub mod logs;
pub mod parameters;

pub fn draw(f: &mut Frame, app: &App) {
    if app.show_help {
        draw_help(f, app);
        return;
    }

    match &app.view {
        View::Dashboard => dashboard::draw(f, app, f.area()),
        View::ServiceDetails(name) => {
            details::draw(f, app, f.area(), name);
        }
        View::Logs(name) => {
            logs::draw(f, app, f.area(), name);
        }
        View::DependencyGraph => {
            graph::draw(f, app, f.area());
        }
        View::Parameters => {
            parameters::draw(f, app, f.area());
        }
    }
}

fn draw_help(f: &mut Frame, _app: &App) {
    use ratatui::{
        layout::{Alignment, Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Paragraph, Wrap},
    };

    let area = f.area();

    let text = vec![
        Line::from(vec![Span::styled(
            "Service Federation TUI - Keyboard Shortcuts",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Global",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  q       Quit"),
        Line::from("  ?       Toggle this help"),
        Line::from("  Esc     Back to dashboard / Close"),
        Line::from("  Ctrl+C  Force quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Dashboard",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ↑/k     Select previous service"),
        Line::from("  ↓/j     Select next service"),
        Line::from("  Enter   View service details"),
        Line::from("  Space   Toggle service (start/stop)"),
        Line::from("  r       Restart selected service"),
        Line::from("  s       Stop all services"),
        Line::from("  S       Start all services"),
        Line::from("  l       View logs"),
        Line::from("  d       View details"),
        Line::from("  g       View dependency graph"),
        Line::from("  p       View parameters"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Logs View",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ↑/↓     Scroll logs"),
        Line::from("  PgUp/Dn Page scroll"),
        Line::from("  Home/g  Jump to start"),
        Line::from("  End/G   Jump to end"),
        Line::from("  f       Toggle follow mode"),
        Line::from("  /       Search logs"),
        Line::from("  c       Clear logs"),
        Line::from("  e       Jump to next error"),
        Line::from("  1-4     Filter log level (1=error, 4=debug)"),
        Line::from("  0       Show all levels"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press ? or Esc to close",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);

    // Center the help box
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(area);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(chunks[1]);

    f.render_widget(paragraph, h_chunks[1]);
}
