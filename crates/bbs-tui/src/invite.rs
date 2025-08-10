use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(7),
                    Constraint::Min(1),
                    Constraint::Length(3),
                ])
                .split(size);

            let banner_color = match phase % 3 {
                0 => Color::Cyan,
                1 => Color::Magenta,
                _ => Color::Blue,
            };
            let banner = Paragraph::new(vec![
                Line::from(Span::styled("  ____  ____   _____", Style::default().fg(banner_color).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(" | __ )| __ ) | ____|  invite-only", Style::default().fg(banner_color))),
                Line::from(Span::styled(r" |  _ \|  _ \ |  _|", Style::default().fg(banner_color))),
                Line::from(Span::styled(" | |_) | |_) || |___", Style::default().fg(banner_color))),
                Line::from(Span::styled(" |____/|____/ |_____|", Style::default().fg(banner_color))),
            ])
            .block(Block::default().borders(Borders::NONE));
            f.render_widget(banner, chunks[0]);

            let input_label = format!("invite code: {}", input);
            let body = Paragraph::new(input_label)
                .block(Block::default().borders(Borders::ALL).title("access required"));
            f.render_widget(body, chunks[1]);

            let footer = Paragraph::new(Span::styled(
                status.as_str(),
                Style::default().fg(Color::Gray),
            ));
            f.render_widget(footer, chunks[2]);
        })?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout)? {
            if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
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
                    (KeyCode::Char(ch), KeyModifiers::NONE) | (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                        if input.len() < 64 {
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
