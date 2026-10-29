/// Splits `text` into chunks of `size` characters, advancing `step` each
/// time. It works on characters, not bytes, so UTF-8 is never cut in half.
pub fn split(text: &str, size: usize, step: usize) -> Vec<String> {
    assert!(size > 0 && step > 0, "size and step must be positive");
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let end = (start + size).min(chars.len());
        out.push(chars[start..end].iter().collect());
        if end >= chars.len() {
            break;
        }
        start += step;
    }
    out
}
