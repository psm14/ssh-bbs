// rename validation + audit

pub fn valid_nick(name: &str) -> bool {
    let s = name.trim();
    if s.len() < 2 || s.len() > 16 {
        return false;
    }
    s.chars()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nick_validation() {
        assert!(valid_nick("ab"));
        assert!(valid_nick("user_name-1"));
        assert!(!valid_nick("a"));
        assert!(!valid_nick("UPPER"));
        assert!(!valid_nick("bad!name"));
        assert!(!valid_nick("this_name_is_way_too_long"));
    }
}
