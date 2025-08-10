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
use crate::rate::TokenBucket;
use crate::realtime;
use crate::rooms::valid_room_name;
use crate::util::normalize_message;
use std::collections::HashSet;
use tokio::sync::mpsc;

pub struct UiOpts {
    pub history_load: u32,
    pub msg_max_len: usize,
    pub fp_short: String,
    pub rate_per_min: u32,
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
    rooms: Vec<RoomEntry>,
    running: bool,
    bucket: TokenBucket,
}

#[derive(Debug, Clone)]
struct RoomEntry {
    id: i64,
    name: String,
    unread: usize,
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
    let bucket = TokenBucket::new(opts.rate_per_min);
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
        rooms: vec![],
        bucket,
    };
    for m in &app.messages {
        app.seen_ids.insert(m.id);
    }

    // load rooms list (only rooms the user has joined)
    let list = data::list_joined_rooms(&app.pool, app.user.id).await?;
    app.rooms = list
        .into_iter()
        .map(|r| RoomEntry {
            id: r.id,
            name: r.name,
            unread: 0,
        })
        .collect();
    if !app.rooms.iter().any(|r| r.id == app.room.id) {
        app.rooms.push(RoomEntry {
            id: app.room.id,
            name: app.room.name.clone(),
            unread: 0,
        });
    }

    // realtime listener
    let (tx, mut rx) = mpsc::channel::<realtime::Event>(128);
    realtime::spawn_listener(app.pool.clone(), tx).await;

    // event loop
    while app.running {
        // refresh rate bucket view
        let tokens_left = app.bucket.peek_tokens().floor() as i32;
        let tokens_cap = app.bucket.capacity().round() as i32;
        draw(&mut terminal, &app, tokens_left, tokens_cap)?;
        // drain realtime events
        while let Ok(ev) = rx.try_recv() {
            match ev {
                realtime::Event::Message { id, room_id } => {
                    if room_id == app.room.id {
                        if let Some(v) = data::message_view_by_id(&app.pool, id).await? {
                            if !app.seen_ids.contains(&v.id) {
                                app.seen_ids.insert(v.id);
                                app.messages.push(v);
                            }
                        }
                    } else if let Some(re) = app.rooms.iter_mut().find(|r| r.id == room_id) {
                        re.unread = re.unread.saturating_add(1);
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
    let w = terminal.backend_mut();
    crossterm::execute!(w, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &App,
    tokens_left: i32,
    tokens_cap: i32,
) -> Result<()> {
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
            "{} @ {} | msgs:{} | rate:{}/{} | fp:{}",
            app.user.handle,
            app.room.name,
            app.messages.len(),
            tokens_left,
            tokens_cap,
            app.opts.fp_short
        );
        let status = Paragraph::new(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        f.render_widget(status, chunks[0]);

        // messages pane split main + sidebar
        let msg_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(24)])
            .split(chunks[1]);

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
        f.render_widget(messages, msg_chunks[0]);

        // sidebar rooms
        let side_lines: Vec<Line> = app
            .rooms
            .iter()
            .map(|r| {
                let cur = if r.id == app.room.id { '>' } else { ' ' };
                if r.unread > 0 {
                    Line::from(format!("{} {} ({})", cur, r.name, r.unread))
                } else {
                    Line::from(format!("{} {}", cur, r.name))
                }
            })
            .collect();
        let sidebar =
            Paragraph::new(side_lines).block(Block::default().borders(Borders::ALL).title("rooms"));
        f.render_widget(sidebar, msg_chunks[1]);

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
            // normalize body (nfkc + strip controls)
            let s = normalize_message(s);
            // client-side rate bucket
            if !app.bucket.try_consume(1.0) {
                app.status = "rate limited (client)".into();
                app.input.clear();
                return Ok(());
            }
            // send
            let res = data::insert_message(&app.pool, app.room.id, app.user.id, &s).await;
            let msg = match res {
                Ok(m) => m,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("rate_limited") {
                        app.status = "rate limited (server)".into();
                        return Ok(());
                    } else {
                        return Err(e);
                    }
                }
            };
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
            if !app.rooms.is_empty() {
                if let Some(idx) = app.rooms.iter().position(|r| r.id == app.room.id) {
                    let next = (idx + 1) % app.rooms.len();
                    let target = app.rooms[next].id;
                    if let Some(re) = app.rooms.iter().find(|r| r.id == target) {
                        let room =
                            data::ensure_room_exists(&app.pool, &re.name, app.user.id).await?;
                        data::join_room(&app.pool, room.id, app.user.id).await?;
                        app.room = room;
                        app.messages = data::recent_messages_view(
                            &app.pool,
                            app.room.id,
                            app.opts.history_load as i64,
                        )
                        .await?;
                        app.seen_ids.clear();
                        for m in &app.messages {
                            app.seen_ids.insert(m.id);
                        }
                        if let Some(rm) = app.rooms.iter_mut().find(|r| r.id == target) {
                            rm.unread = 0;
                        }
                        app.status = format!("joined {}", app.room.name);
                    }
                }
            }
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
        Command::Quit => {
            app.running = false;
        }
        Command::Me(action) => {
            if action.trim().is_empty() {
                app.status = "usage: /me <action>".into();
                return Ok(());
            }
            let body = format!("* {} {}", app.user.handle, normalize_message(action.trim()));
            let msg = data::insert_message(&app.pool, app.room.id, app.user.id, &body).await?;
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
            app.status = "me".into();
        }
        Command::Nick(new) => {
            let new = new.trim();
            if !valid_nick(new) {
                app.status = "invalid nick [a-z0-9_-]{2,16}".into();
                return Ok(());
            }
            match data::change_handle(&app.pool, app.user.id, new).await {
                Ok(updated) => {
                    app.user = updated;
                    app.status = "nick changed".into();
                }
                Err(e) => {
                    let is_unique = e
                        .downcast_ref::<sqlx::Error>()
                        .and_then(|err| err.as_database_error())
                        .and_then(|d| d.code())
                        .map(|c| c == "23505")
                        .unwrap_or(false);
                    if is_unique {
                        app.status = "nick taken".into();
                    } else {
                        app.status = format!("nick error: {}", e);
                    }
                }
            }
        }
        Command::Join(name) => {
            let name = name.trim();
            if !valid_room_name(name) {
                app.status = "invalid room [a-z0-9_-]{1,24}".into();
                return Ok(());
            }
            let room = match data::ensure_room_exists(&app.pool, name, app.user.id).await {
                Ok(r) => r,
                Err(e) => {
                    if e.to_string().contains("room_deleted") {
                        app.status = "room is deleted".into();
                        return Ok(());
                    }
                    return Err(e);
                }
            };
            data::join_room(&app.pool, room.id, app.user.id).await?;
            app.room = room;
            app.messages =
                data::recent_messages_view(&app.pool, app.room.id, app.opts.history_load as i64)
                    .await?;
            app.seen_ids.clear();
            for m in &app.messages {
                app.seen_ids.insert(m.id);
            }
            if let Some(rm) = app.rooms.iter_mut().find(|r| r.id == app.room.id) {
                rm.unread = 0;
            }
            if !app.rooms.iter().any(|r| r.id == app.room.id) {
                app.rooms.push(RoomEntry {
                    id: app.room.id,
                    name: app.room.name.clone(),
                    unread: 0,
                });
            }
            app.status = "joined".into();
        }
        Command::RoomDel(name) => {
            let name = name.trim();
            if !valid_room_name(name) {
                app.status = "usage: /roomdel <name> (a-z0-9_-){1,24}".into();
                return Ok(());
            }
            let ok = data::soft_delete_room_by_creator(&app.pool, name, app.user.id).await?;
            if ok {
                app.status = format!("room '{}' deleted", name);
                // refresh rooms list (joined rooms)
                let list = data::list_joined_rooms(&app.pool, app.user.id).await?;
                app.rooms = list
                    .into_iter()
                    .map(|r| RoomEntry {
                        id: r.id,
                        name: r.name,
                        unread: 0,
                    })
                    .collect();
            } else {
                app.status = "not room creator or already deleted".into();
            }
        }
        Command::Leave(name_opt) => {
            // Determine room to leave
            let target_room_name_owned = name_opt.unwrap_or_else(|| app.room.name.clone());
            let target_name = target_room_name_owned.trim();
            if target_name.is_empty() {
                app.status = "usage: /leave [room]".into();
                return Ok(());
            }
            // Find room entry by name
            if let Some(idx) = app.rooms.iter().position(|r| r.name == target_name) {
                let leaving_id = app.rooms[idx].id;
                let leaving_is_current = leaving_id == app.room.id;

                if leaving_is_current {
                    // Need another room to focus
                    if app.rooms.len() <= 1 {
                        app.status = "cannot leave the last room".into();
                        return Ok(());
                    }
                    // Drop membership first
                    let _ = data::leave_room(&app.pool, leaving_id, app.user.id).await?;
                    // pick next room different from current
                    let mut candidate = None;
                    for off in 0..app.rooms.len() {
                        let j = (idx + 1 + off) % app.rooms.len();
                        if app.rooms[j].id != leaving_id {
                            candidate = Some(app.rooms[j].id);
                            break;
                        }
                    }
                    if let Some(next_id) = candidate {
                        // load next room by id (name lookup from list)
                        if let Some(re) = app.rooms.iter().find(|r| r.id == next_id) {
                            let room =
                                data::ensure_room_exists(&app.pool, &re.name, app.user.id).await?;
                            data::join_room(&app.pool, room.id, app.user.id).await?;
                            app.room = room;
                            app.messages = data::recent_messages_view(
                                &app.pool,
                                app.room.id,
                                app.opts.history_load as i64,
                            )
                            .await?;
                            app.seen_ids.clear();
                            for m in &app.messages {
                                app.seen_ids.insert(m.id);
                            }
                        }
                    }
                    // remove leaving room from sidebar
                    if let Some(idx2) = app.rooms.iter().position(|r| r.id == leaving_id) {
                        app.rooms.remove(idx2);
                    }
                    app.status = format!("left '{}'", target_name);
                } else {
                    // Leaving a non-focused room: drop membership and remove from sidebar
                    let _ = data::leave_room(&app.pool, leaving_id, app.user.id).await?;
                    app.rooms.remove(idx);
                    app.status = format!("left '{}'", target_name);
                }
            } else {
                app.status = "room not in sidebar".into();
            }
        }
        Command::Rooms => {
            // Show joined rooms with join times; mark current with '>'
            let list = data::list_joined_rooms_with_times(&app.pool, app.user.id).await?;
            if list.is_empty() {
                app.status = "rooms: (none)".into();
            } else {
                let items: Vec<String> = list
                    .into_iter()
                    .map(|r| {
                        let mark = if r.id == app.room.id { "> " } else { "" };
                        let ts = r.last_joined_at.format("%H:%M");
                        format!("{}{} [{}]", mark, r.name, ts)
                    })
                    .collect();
                app.status = format!("rooms: {}", items.join(", "));
            }
        }
        Command::Who(_room) => {
            let who = data::list_recent_members(&app.pool, app.room.id, 50).await?;
            let names: Vec<String> = who.into_iter().map(|u| u.handle).collect();
            app.status = format!("who: {}", names.join(", "));
        }
    }
    Ok(())
}
