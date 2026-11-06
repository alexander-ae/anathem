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

/// True for the scripts written without spaces between words: Han
/// ideographs, hiragana and katakana.
///
/// Hangul is deliberately **not** here. Korean is written with spaces, so
/// `unicode61` already splits it into words, and bigramming it would break
/// what works today.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF |   // hiragana, katakana
        0x31F0..=0x31FF |   // katakana phonetic extensions
        0x3400..=0x4DBF |   // CJK unified ideographs extension A
        0x4E00..=0x9FFF |   // CJK unified ideographs
        0xF900..=0xFAFF |   // CJK compatibility ideographs
        0xFF66..=0xFF9D |   // halfwidth katakana
        0x20000..=0x2A6DF   // CJK unified ideographs extension B
    )
}

/// Rewrites runs of CJK characters as overlapping bigrams, so FTS5 indexes
/// them as several tokens instead of one.
///
/// `unicode61` splits on whitespace and punctuation, and a Japanese or
/// Chinese sentence has neither inside it: a whole clause arrives as a
/// single token, and a search for anything within it matches nothing — the
/// way `figur` does not find `configuration`. Bigrams are what Lucene's
/// `CJKBigramFilter` uses for the same reason. `設定ファイル` becomes
/// `設定 定フ ファ ァイ イル`, and a query for `設定` then has a token to hit.
///
/// Text with no CJK in it comes back unchanged, which matters more than it
/// looks: every figure this project publishes was measured on Latin text,
/// and a transformation that touched it would move all of them.
///
/// The same function has to run over the query, or the two sides would be
/// tokenized differently and nothing would ever match.
pub fn segment_cjk(text: &str) -> String {
    if !text.chars().any(is_cjk) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() * 2);
    let mut run: Vec<char> = Vec::new();

    fn flush(out: &mut String, run: &mut Vec<char>) {
        if run.is_empty() {
            return;
        }
        if !out.is_empty() && !out.ends_with(char::is_whitespace) {
            out.push(' ');
        }
        if run.len() == 1 {
            out.push(run[0]);
        } else {
            for (i, w) in run.windows(2).enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push(w[0]);
                out.push(w[1]);
            }
        }
        run.clear();
    }

    for c in text.chars() {
        if is_cjk(c) {
            run.push(c);
        } else {
            flush(&mut out, &mut run);
            // A bigram run that ends flush against a Latin word would glue
            // the two into one token again, so the boundary gets a space.
            if !c.is_whitespace()
                && !out.is_empty()
                && !out.ends_with(char::is_whitespace)
                && out.chars().next_back().is_some_and(is_cjk)
            {
                out.push(' ');
            }
            out.push(c);
        }
    }
    flush(&mut out, &mut run);
    out
}
