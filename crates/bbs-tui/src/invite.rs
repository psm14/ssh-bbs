use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::layout::Alignment;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use sqlx::PgPool;
use std::io;
use std::time::{Duration, Instant};

pub async fn prompt(pool: &PgPool) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut input = String::new();
    let mut status = String::from("Enter invite code to join");
    let mut last_tick = Instant::now();
    let mut phase = 0u8;

    loop {
        terminal.draw(|f| {
            let size = f.size();
            // Use 4 chunks: top padding, banner, input area, bottom padding.
            // This centers the input area vertically while keeping the banner
            // and padding consistent.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1), // top padding
                    Constraint::Length(7), // banner
                    Constraint::Length(3), // input area (single line)
                    Constraint::Min(1), // bottom padding
                ])
                .split(size);

            let banner_color = match phase % 3 {
                0 => Color::Cyan,
                1 => Color::Magenta,
                _ => Color::Blue,
            };
            let banner = Paragraph::new(vec![
                Line::from(Span::styled(
                    "  ____  ____  _____  ",
                    Style::default()
                        .fg(banner_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    " | __ )| __ )| ____| ",
                    Style::default().fg(banner_color),
                )),
                Line::from(Span::styled(
                    r" |  _ \|  _ \|  _|   ",
                    Style::default().fg(banner_color),
                )),
                Line::from(Span::styled(
                    " | |_) | |_) | |___  ",
                    Style::default().fg(banner_color),
                )),
                Line::from(Span::styled(
                    " |____/|____/|_____| ",
                    Style::default().fg(banner_color),
                )),
            ])
            .block(Block::default().borders(Borders::NONE));
            f.render_widget(banner.alignment(Alignment::Center), chunks[1]);

            // Render a single-line, 16-char wide input box centered vertically.
            // The box is horizontally centered by splitting the input chunk into
            // left/middle/right sections and only rendering the middle section.
            let inner_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(16),
                    Constraint::Min(1),
                ])
                .split(chunks[2]);

            let body = Paragraph::new(input.clone())
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);
            f.render_widget(body, inner_chunks[1]);
        })?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout)? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match (code, modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        cleanup(&mut terminal)?;
                        return Err(anyhow!("cancelled"));
                    }
                    (KeyCode::Esc, _) => {
                        cleanup(&mut terminal)?;
                        return Err(anyhow!("cancelled"));
                    }
                    (KeyCode::Backspace, _) => {
                        input.pop();
                    }
                    (KeyCode::Enter, _) => {
                        let code = input.trim();
                        if code.is_empty() {
                            status = "enter a code".into();
                        } else {
                            match crate::data::consume_invite(pool, code).await {
                                Ok(true) => {
                                    cleanup(&mut terminal)?;
                                    return Ok(());
                                }
                                Ok(false) => {
                                    status = "invalid code".into();
                                    input.clear();
                                }
                                Err(e) => {
                                    status = format!("error: {}", e);
                                }
                            }
                        }
                    }
                    (KeyCode::Char(ch), KeyModifiers::NONE)
                    | (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                        // Invite codes are max 16 characters
                        if input.len() < 16 {
                            input.push(ch);
                        }
                    }
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(250) {
            phase = phase.wrapping_add(1);
            last_tick = Instant::now();
        }
    }
}

fn cleanup(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    let w = terminal.backend_mut();
    crossterm::execute!(w, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
