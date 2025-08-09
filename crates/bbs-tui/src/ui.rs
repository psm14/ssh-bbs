use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use sqlx::PgPool;
use std::{io, time::Duration};

use crate::data::{self, MessageView, Room, User};
use crate::input::{parse_command, Command};
use crate::nick::valid_nick;
use crate::rooms::valid_room_name;
use crate::realtime;
use std::collections::HashSet;
use tokio::sync::mpsc;

pub struct UiOpts {
    pub history_load: u32,
    pub msg_max_len: usize,
    pub fp_short: String,
}

struct App {
    pool: PgPool,
    user: User,
    room: Room,
    opts: UiOpts,
    input: String,
    status: String,
    messages: Vec<MessageView>,
    seen_ids: HashSet<i64>,
    running: bool,
}

pub async fn run(pool: PgPool, user: User, room: Room, opts: UiOpts) -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, DisableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.show_cursor()?;

    // preload messages
    let mut app = App {
        messages: data::recent_messages_view(&pool, room.id, opts.history_load as i64).await?,
        pool,
        user,
        room,
        opts,
        input: String::new(),
        status: String::from("/help for commands"),
        running: true,
        seen_ids: HashSet::new(),
    };
    for m in &app.messages {
        app.seen_ids.insert(m.id);
    }

    // realtime listener
    let (tx, mut rx) = mpsc::channel::<realtime::Event>(128);
    realtime::spawn_listener(app.pool.clone(), app.room.id, tx).await;

    // event loop
    while app.running {
        draw(&mut terminal, &app)?;
        // drain realtime events
        while let Ok(ev) = rx.try_recv() {
            match ev {
                realtime::Event::Message { id, .. } => {
                    if let Some(v) = data::message_view_by_id(&app.pool, id).await? {
                        if !app.seen_ids.contains(&v.id) {
                            app.seen_ids.insert(v.id);
                            app.messages.push(v);
                        }
                    }
                }
            }
        }
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(k) = event::read()? {
                handle_key(&mut app, k).await?;
            }
        }
    }

    // restore terminal
    disable_raw_mode()?;
    let mut w = terminal.backend_mut();
    crossterm::execute!(w, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &App) -> Result<()> {
    terminal.draw(|f| {
        let size = f.size();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(size);

        // status line
        let title = format!(
            "{} @ {} | msgs:{} | fp:{}",
            app.user.handle,
            app.room.name,
            app.messages.len(),
            app.opts.fp_short
        );
        let status = Paragraph::new(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        f.render_widget(status, chunks[0]);

        // messages pane
        let lines: Vec<Line> = app
            .messages
            .iter()
            .map(|m| {
                let ts = m.created_at.format("%H:%M:%S");
                Line::from(format!("[{}] {}: {}", ts, m.user_handle, sanitize(&m.body)))
            })
            .collect();
        let messages =
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("messages"));
        f.render_widget(messages, chunks[1]);

        // input line
        let input = Paragraph::new(app.input.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.status.as_str()),
        );
        f.render_widget(input, chunks[2]);
    })?;
    Ok(())
}

async fn handle_key(app: &mut App, k: KeyEvent) -> Result<()> {
    match (k.code, k.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.running = false;
        }
        (KeyCode::Esc, _) => {
            app.input.clear();
        }
        (KeyCode::Backspace, _) => {
            app.input.pop();
        }
        (KeyCode::Enter, _) => {
            let s = app.input.trim();
            if s.is_empty() {
                app.status = "empty".into();
                app.input.clear();
                return Ok(());
            }
            if let Some(cmd) = parse_command(s) {
                handle_command(app, cmd).await?;
                app.input.clear();
                return Ok(());
            }
            if s.len() > app.opts.msg_max_len {
                return Err(anyhow!("message too long"));
            }
            // send
            let msg = data::insert_message(&app.pool, app.room.id, app.user.id, s).await?;
            let mv = MessageView {
                id: msg.id,
                room_id: msg.room_id,
                user_id: msg.user_id,
                user_handle: app.user.handle.clone(),
                body: msg.body,
                created_at: msg.created_at,
            };
            app.seen_ids.insert(mv.id);
            app.messages.push(mv);
            app.status = "sent".into();
            app.input.clear();
        }
        (KeyCode::Char(ch), KeyModifiers::NONE) | (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
            app.input.push(ch);
        }
        (KeyCode::Tab, _) => {
            // reserved for room switch later
        }
        _ => {}
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

async fn handle_command(app: &mut App, cmd: Command) -> Result<()> {
    match cmd {
        Command::Help => {
            app.status = "/help /quit /nick /join /rooms /who /me".into();
        }
        Command::Quit => { app.running = false; }
        Command::Me(action) => {
            if action.trim().is_empty() { app.status = "usage: /me <action>".into(); return Ok(()); }
            let body = format!("* {} {}", app.user.handle, action.trim());
            let msg = data::insert_message(&app.pool, app.room.id, app.user.id, &body).await?;
            let mv = MessageView { id: msg.id, room_id: msg.room_id, user_id: msg.user_id, user_handle: app.user.handle.clone(), body: msg.body, created_at: msg.created_at };
            app.seen_ids.insert(mv.id);
            app.messages.push(mv);
            app.status = "me".into();
        }
        Command::Nick(new) => {
            let new = new.trim();
            if !valid_nick(new) { app.status = "invalid nick [a-z0-9_-]{2,16}".into(); return Ok(()); }
            match data::change_handle(&app.pool, app.user.id, new).await {
                Ok(updated) => { app.user = updated; app.status = "nick changed".into(); }
                Err(e) => {
                    let is_unique = e.downcast_ref::<sqlx::Error>().and_then(|err| err.as_database_error()).and_then(|d| d.code()).map(|c| c == "23505").unwrap_or(false);
                    if is_unique { app.status = "nick taken".into(); } else { app.status = format!("nick error: {}", e).into(); }
                }
            }
        }
        Command::Join(name) => {
            let name = name.trim();
            if !valid_room_name(name) { app.status = "invalid room [a-z0-9_-]{1,24}".into(); return Ok(()); }
            let room = data::ensure_room_exists(&app.pool, name, app.user.id).await?;
            data::join_room(&app.pool, room.id, app.user.id).await?;
            app.room = room;
            app.messages = data::recent_messages_view(&app.pool, app.room.id, app.opts.history_load as i64).await?;
            app.seen_ids.clear();
            for m in &app.messages { app.seen_ids.insert(m.id); }
            app.status = "joined".into();
        }
        Command::Leave(_name) => {
            app.status = "left (ui only)".into();
        }
        Command::Rooms => {
            let rooms = data::list_rooms(&app.pool).await?;
            let names: Vec<String> = rooms.into_iter().map(|r| r.name).collect();
            app.status = format!("rooms: {}", names.join(", "));
        }
        Command::Who(_room) => {
            let who = data::list_recent_members(&app.pool, app.room.id, 50).await?;
            let names: Vec<String> = who.into_iter().map(|u| u.handle).collect();
            app.status = format!("who: {}", names.join(", "));
        }
    }
    Ok(())
}
