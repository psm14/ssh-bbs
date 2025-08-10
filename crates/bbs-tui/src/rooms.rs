// join/leave/history validators

pub fn valid_room_name(name: &str) -> bool {
    let s = name.trim();
    if s.is_empty() || s.len() > 24 {
        return false;
    }
    s.chars()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_name_validation() {
        assert!(valid_room_name("lobby"));
        assert!(valid_room_name("dev_chat-1"));
        assert!(!valid_room_name(""));
        assert!(!valid_room_name("TOO_BIG_AND_UPPER"));
        assert!(!valid_room_name("bad*chars"));
    }
}
