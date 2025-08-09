// command parsing + keybinds

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Quit,
    Me(String),
    Nick(String),
    Join(String),
    Leave(Option<String>),
    Rooms,
    Who(Option<String>),
}

pub fn parse_command(s: &str) -> Option<Command> {
    let s = s.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = &s[1..];
    let mut parts = rest.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim().to_string();
    match cmd {
        "help" | "h" | "?" => Some(Command::Help),
        "quit" | "q" | "exit" => Some(Command::Quit),
        "me" => Some(Command::Me(arg)),
        "nick" | "name" => Some(Command::Nick(arg)),
        "join" => Some(Command::Join(arg)),
        "leave" => Some(Command::Leave(if arg.is_empty() {
            None
        } else {
            Some(arg)
        })),
        "rooms" => Some(Command::Rooms),
        "who" => Some(Command::Who(if arg.is_empty() { None } else { Some(arg) })),
        _ => Some(Command::Help),
    }
}
