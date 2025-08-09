// join/leave/history validators

pub fn valid_room_name(name: &str) -> bool {
    let s = name.trim();
    if s.is_empty() || s.len() > 24 { return false; }
    s.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}
