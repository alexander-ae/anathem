/// Parte `text` en fragmentos de `size` caracteres avanzando `step` cada vez.
/// Opera sobre caracteres, no bytes, para no cortar UTF-8 a la mitad.
pub fn split(text: &str, size: usize, step: usize) -> Vec<String> {
    assert!(size > 0 && step > 0, "size y step deben ser positivos");
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
