use anyhow::Result;
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;

pub async fn run() -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, DisableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // initial draw
    draw_ui(&mut terminal)?;

    // wait for ctrl+c then cleanup
    let _ = tokio::signal::ctrl_c().await;

    // restore terminal
    disable_raw_mode()?;
    // LeaveAlternateScreen must be executed on the same writer used by terminal
    let mut w = terminal.backend_mut();
    crossterm::execute!(w, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw_ui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    terminal.draw(|f| {
        let size = f.size();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // status line
                Constraint::Min(1),    // messages
                Constraint::Length(3), // input
            ])
            .split(size);

        // status line
        let status = Paragraph::new(Span::styled(
            "bbs-tui — connected (Ctrl+C to quit)",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        f.render_widget(status, chunks[0]);

        // messages pane placeholder
        let messages = Block::default().borders(Borders::ALL).title("messages");
        f.render_widget(messages, chunks[1]);

        // input line placeholder
        let input = Block::default().borders(Borders::ALL).title("input");
        f.render_widget(input, chunks[2]);
    })?;
    Ok(())
}
