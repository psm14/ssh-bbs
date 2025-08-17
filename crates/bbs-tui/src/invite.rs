use crate::life::{Life, LifeWidget};
use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Alignment;
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
    let mut last_tick = Instant::now();
    let mut last_step = Instant::now();
    let mut phase = 0u8;
    // Initialize Life background sized to current terminal
    let mut last_size = terminal.size()?;
    let mut life = Life::new(last_size.width as usize, last_size.height as usize);

    loop {
        terminal.draw(|f| {
            let size = f.size();
            // Resize life grid if terminal size changed
            if size != last_size { /* resized */ }
            // Render animated life background first
            let life_widget = LifeWidget::new(&life);
            f.render_widget(life_widget, size);
            // Use 4 chunks: top padding, banner, input area, bottom padding.
            // This centers the input area vertically while keeping the banner
            // and padding consistent.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),    // top padding
                    Constraint::Length(7), // banner
                    Constraint::Length(3), // input area (single line)
                    Constraint::Min(1),    // bottom padding
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

            // Center a 16-char input field with a 3-row bordered box (height 3)
            // Width 18 to account for borders on both sides.
            let inner = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(18),
                    Constraint::Min(1),
                ])
                .split(chunks[2]);
            let body = Paragraph::new(input.clone())
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);
            f.render_widget(body, inner[1]);
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
                        if !code.is_empty() {
                            match crate::data::consume_invite(pool, code).await {
                                Ok(true) => {
                                    cleanup(&mut terminal)?;
                                    return Ok(());
                                }
                                Ok(false) => {
                                    // invalid code: clear input but show no status
                                    input.clear();
                                }
                                Err(_e) => {
                                    // error: ignore visual status; keep input for retry
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
        // Step the life simulation at ~12 FPS
        if last_step.elapsed() >= Duration::from_millis(80) {
            // handle terminal resize for life grid
            let sz = terminal.size()?;
            if sz != last_size {
                life.resize(sz.width as usize, sz.height as usize);
                last_size = sz;
            }
            life.step();
            life.maybe_spawn();
            last_step = Instant::now();
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
