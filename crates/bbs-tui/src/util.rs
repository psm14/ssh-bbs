// fp shortener, formatting utilities

pub fn fp_short(fp_b64: &str) -> String {
    // show first 8 chars of ssh-style base64 sha256
    let s = fp_b64.trim();
    s.chars().take(8).collect()
}

// Normalize message bodies: NFKC + strip control chars except \n and \t
pub fn normalize_message(input: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = input.nfkc().collect();
    normalized
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
