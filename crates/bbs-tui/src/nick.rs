// rename validation + audit

pub fn valid_nick(name: &str) -> bool {
    let s = name.trim();
    if s.len() < 2 || s.len() > 16 {
        return false;
    }
    s.chars()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}
