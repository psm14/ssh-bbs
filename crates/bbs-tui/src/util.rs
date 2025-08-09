// fp shortener, formatting utilities (to be implemented)

pub fn fp_short(fp_b64: &str) -> String {
    // show first 8 chars of ssh-style base64 sha256
    let s = fp_b64.trim();
    s.chars().take(8).collect()
}
