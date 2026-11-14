//! What a terminal is allowed to interpret.
//!
//! A memory body is untrusted text. It can carry `\r`, which returns the
//! cursor and makes a line say what it does not, and ESC sequences, which a
//! terminal obeys: erase the screen, hide text, set the clipboard, or draw a
//! link whose target is not the text shown.
//!
//! The data is not the problem and is not touched — the note is stored as it
//! was written. What is decided here is what reaches a terminal. Nothing
//! runs when the output is piped, so a script still gets the exact bytes.

use std::io::IsTerminal;

/// Caret notation for one control character, the way `cat -v` shows it.
fn caret(c: char) -> String {
    match c as u32 {
        0x7f => "^?".to_string(),
        n @ 0x00..=0x1f => format!("^{}", char::from_u32(n + 0x40).unwrap_or('?')),
        _ => "?".to_string(),
    }
}

/// `text` with every control character made visible, except the two a
/// terminal is expected to act on: newline and tab.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\t' => out.push(c),
            c if c.is_control() => out.push_str(&caret(c)),
            c => out.push(c),
        }
    }
    out
}

/// What actually goes out: the text untouched when it is going to a pipe
/// or `--raw` asked for it, cleaned when a person is looking at it.
pub fn render(text: &str, raw: bool, tty: bool) -> String {
    if raw || !tty {
        text.to_string()
    } else {
        sanitize(text)
    }
}

/// Prints `text` to stdout by the rule above.
///
/// `raw` is `--raw`: the escape hatch for whoever wants the bytes, and the
/// reason this is not a decision taken on their behalf.
pub fn print(text: &str, raw: bool) {
    print!("{}", render(text, raw, std::io::stdout().is_terminal()));
}

/// `print`, plus the newline.
pub fn print_line(text: &str, raw: bool) {
    print(text, raw);
    println!();
}

#[cfg(test)]
mod tests {
    use super::{render, sanitize};

    #[test]
    fn plain_text_is_untouched() {
        let t = "the payment provider is Adyen\n\tsince October";
        assert_eq!(sanitize(t), t);
    }

    #[test]
    fn a_carriage_return_does_not_rewrite_the_line() {
        assert_eq!(sanitize("balance 100\rTRANSFER DONE"), "balance 100^MTRANSFER DONE");
    }

    #[test]
    fn an_escape_sequence_becomes_visible() {
        assert_eq!(sanitize("\u{1b}[2Jgone"), "^[[2Jgone");
    }

    #[test]
    fn an_osc_link_loses_its_control() {
        assert_eq!(sanitize("a\u{1b}]8;;http://x\u{1b}\\b"), "a^[]8;;http://x^[\\b");
    }

    #[test]
    fn other_controls_and_delete_are_marked() {
        assert_eq!(sanitize("a\u{7}b\u{7f}c"), "a^Gb^?c");
    }

    #[test]
    fn a_pipe_and_raw_both_keep_the_bytes() {
        let t = "a\rb\u{1b}[2J";
        assert_eq!(render(t, false, false), t, "a pipe gets exactly what is stored");
        assert_eq!(render(t, true, true), t, "--raw gets it too, even on a terminal");
        assert_ne!(render(t, false, true), t, "a terminal is the one case that changes");
    }
}
