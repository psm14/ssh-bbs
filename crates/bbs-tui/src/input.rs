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
    RoomDel(String),
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
        "roomdel" | "rdel" => Some(Command::RoomDel(arg)),
        _ => Some(Command::Help),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nick_join_me() {
        assert_eq!(
            parse_command("/nick alice"),
            Some(Command::Nick("alice".into()))
        );
        assert_eq!(
            parse_command("/join lobby"),
            Some(Command::Join("lobby".into()))
        );
        assert_eq!(
            parse_command("/me waves"),
            Some(Command::Me("waves".into()))
        );
    }

    #[test]
    fn parses_variants_and_defaults() {
        assert_eq!(parse_command("/help"), Some(Command::Help));
        assert_eq!(parse_command("/who"), Some(Command::Who(None)));
        assert_eq!(parse_command("/leave"), Some(Command::Leave(None)));
        assert_eq!(
            parse_command("/leave lobby"),
            Some(Command::Leave(Some("lobby".into())))
        );
    }
}
