//! No Spanish is left in what anathem prints.
//!
//! This is the failure mode the migration produced three times, and all three
//! had the same shape: a translated layer wrapping one that was not. A server
//! notice with its first line in English and the rest in Spanish;
//! `recent_memories` printing `[tema:` while the other two tools said
//! `[topic:`; a `Not saved:` with the error body in Spanish. None of them was
//! found by whoever wrote the change.
//!
//! It looks at strings only and skips comments: the repository's comments are
//! in English by another route, and mixing the two would fail the guard for
//! reasons that are not its own.

/// Words that do not exist in English and did exist in the Spanish that was
/// here. Deliberately common: what is being hunted is a whole sentence left
/// untranslated, not a stray word.
const TELLTALE: &[&str] = &[
    "memoria", "búsqueda", "guardar", "guardada", "archivo", "paquete",
    "clave", "proyecto", "ventana", "tema", "ninguna", "ningún",
    "no se pudo", "no existe", "está", "tiene", "hay que", "según",
];

/// `tags.rs` holds the default taxonomy's patterns, regexes meant to
/// recognise Spanish text inside memories. They are data, not messages:
/// translating them would change what gets tagged.
const EXEMPT: &[&str] = &["src/tags.rs"];

/// Spanish strings that stay, each for a reason that is not laziness. They
/// are kept apart from `EXEMPT` on purpose: exempting a whole file hides
/// whatever comes after it, and these are few and known.
const ALLOWED: &[(&str, &str)] = &[
    // Compatibility: the key names `config.json` had before 0.2.1, which
    // `serde(alias)` still accepts. Translating them would stop reading the
    // files people already have.
    ("traspaso_items", "config.json alias from before 0.2.1"),
    ("traspaso_ventana", "config.json alias from before 0.2.1"),
    ("soltar_paquete_tras", "config.json alias from before 0.2.1"),
    // Migration: column names the database had and that
    // `rename_columns_to_english` looks for to rename them. If they change, an
    // old database stops migrating.
    ("clave", "column name the migration looks for"),
    ("valor", "column name the migration looks for"),
    ("agente", "column name the migration looks for"),
    ("creado_ts", "column name the migration looks for"),
    // Stored data: the value `memories.project` gets when none can be
    // derived. Changing it would split every existing database's projects in
    // two, which is a data migration and not a translation.
    ("sin-proyecto", "value stored in memories.project"),
    // Delimiters of the handoff block. They are markers, not prose, and
    // changing them improves nothing for whoever reads them: the model only
    // has to see them appear once.
    // Leftovers of an interrupted installation: the name of the side file
    // `replace_file` creates. `doctor` looks for them by this name, so
    // changing it would stop detecting the ones already on disk.
    (".reemplazando-", "file name doctor looks for"),
    // The directory packs live in, on disk: `~/.anathem/paquetes/`.
    // Renaming it would leave every existing installation unable to find its
    // pack, so it is a file migration and not a translation. `replace_file`'s
    // fallback comes along for the same reason.
    ("paquetes", "on-disk directory of existing installations"),
    ("paquete", "fallback of the side file name"),
];

/// The parts of `text` that are inside a string, with their line number.
///
/// It walks the whole file keeping track of whether it is inside a string,
/// instead of looking at each line on its own. The first version did
/// `find('"')` and `rfind('"')` per line and returned nothing when there were
/// not two: it skipped EVERY line of a multi-line string, which is exactly
/// where the remaining Spanish lived. The guard passed green with five whole
/// messages untranslated.
///
/// `'"'` is neutralised before walking: it is a character literal and opens
/// nothing, but the walk would count it as opening.
fn strings_in(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut part = String::new();
    let mut start = 0usize;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.replace("'\"'", "'Q'");
        if !inside && line.trim_start().starts_with("//") {
            continue;
        }
        let mut esc = false;
        for c in line.chars() {
            if esc {
                esc = false;
                if inside { part.push(c); }
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                if inside && !part.trim().is_empty() {
                    // Closes: emit this literal and do not merge it with the
                    // next one on the same line.
                    out.push((start, std::mem::take(&mut part)));
                } else {
                    part.clear();
                    start = n + 1;
                }
                inside = !inside;
            } else if inside {
                part.push(c);
            }
        }
        // A multi-line string is still open: keep this line's part so the
        // message can point at where it is, and keep accumulating.
        if inside && !part.trim().is_empty() {
            out.push((n + 1, std::mem::take(&mut part)));
        }
    }
    out
}

#[test]
fn no_spanish_left_in_any_user_facing_string() {
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir("src").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let as_str = p.to_string_lossy().replace('\\', "/");
        if EXEMPT.iter().any(|e| as_str.ends_with(e)) {
            continue;
        }
        for (n, s) in strings_in(&std::fs::read_to_string(&p).unwrap()) {
            // Exact match, not `contains`: an entry like «clave» compared
            // with `contains` would exempt any future string holding that
            // word, and the eighteen telltales with it. The three that are
            // genuinely fragments sit apart and say why.
            const FRAGMENTS: &[&str] = &[
                // Appears embedded in the side file's name.
                ".reemplazando-",
            ];
            let t = s.trim();
            if ALLOWED.iter().any(|(a, _)| t == *a) || FRAGMENTS.iter().any(|f| s.contains(f)) {
                continue;
            }
            let lower = s.to_lowercase();
            for t in TELLTALE {
                if lower.contains(t) {
                    offenders.push(format!("{}:{} «{t}» in {}", as_str, n, s.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Spanish left in strings the user sees:\n{}",
        offenders.join("\n")
    );
}

/// The same for the scripts. A half-translated installer is worse than one
/// entirely in Spanish: whoever runs it sees half a sentence they understand
/// and half they do not, and cannot tell whether the problem is theirs.
///
/// Here there are no strings to separate from the comments — the whole file
/// is text — so it looks at the whole file except the lines that are clearly
/// identifiers, and relies on Spanish carrying accents far more often than
/// English carries non-ASCII characters.
#[test]
fn no_spanish_left_in_the_scripts() {
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir("scripts").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|x| x != "sh") {
            continue;
        }
        for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
            // The old flags are still accepted in the `case`, on purpose:
            // they are compatibility, not untranslated text.
            if line.contains("--compilar") || line.contains("--etiqueta")
                || line.contains("--paquete") || line.contains("--sin-paquete")
                || line.contains("--configurar-path") || line.contains("--conectar")
                || line.contains("--prefijo") || line.contains("--sin-verificar")
            {
                continue;
            }
            if line.chars().any(|c| "áéíóúñ¿¡Á É Í Ó Ú Ñ".contains(c) && !c.is_whitespace()) {
                offenders.push(format!("{}:{} {}", p.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Spanish left in the scripts:\n{}",
        offenders.join("\n")
    );
}
