//! The documents are in English and describe the program that exists.
//!
//! The second half matters as much as the first. Translating a document is
//! worth nothing if what it says stopped being true, and this migration left
//! exactly that: `AGENTS.md` promised an agent that `mem doctor` ends in
//! «Todo en orden.», and three commits earlier it had started ending in "All
//! good.". An agent checking for that sentence reports a failed installation
//! over a perfect one.
//!
//! This guard was written BEFORE translating, unlike the one over `src/`,
//! which was written at the end and so never found what was already broken.

use std::path::{Path, PathBuf};

/// Flags and strings the program stopped emitting or accepting. A document
/// that names one is teaching something that does not exist.
///
/// The left column stays in Spanish on purpose: it is the old text being
/// searched for, and translating it would make the guard look for something
/// nothing ever wrote.
const GONE: &[(&str, &str)] = &[
    ("--configurar-path", "now --setup-path"),
    ("--conectar", "now --connect"),
    ("--sin-paquete", "now --no-pack"),
    ("--sin-verificar", "now --no-verify"),
    ("--prefijo", "now --prefix"),
    ("--compilar", "now --build"),
    ("--etiqueta", "now --tag"),
    ("Todo en orden", "now \"All good.\""),
    ("--desde", "now --since (recent) or --from (import)"),
    ("--limite", "now --limit"),
    ("--todos", "now --all"),
    ("--fusionar", "now --merge"),
];

/// Record and process, not documentation anybody follows: those stay in
/// Spanish.
fn is_exempt(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    s.contains("docs/historia/") || s.contains("docs/superpowers/")
}

fn living_docs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("README.md"), PathBuf::from("AGENTS.md")];
    for e in std::fs::read_dir("docs").unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "md") && !is_exempt(&p) {
            v.push(p);
        }
    }
    v
}

#[test]
fn no_doc_teaches_something_that_no_longer_exists() {
    let mut offenders = Vec::new();
    for f in living_docs() {
        for (n, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            for (gone, now) in GONE {
                if line.contains(gone) {
                    offenders.push(format!("{}:{} «{gone}» — {now}", f.display(), n + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "documents teaching something that no longer exists:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn no_spanish_left_in_the_docs() {
    let mut offenders = Vec::new();
    for f in living_docs() {
        for (n, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            // A document is prose all the way through, so there is no need
            // to separate strings from comments as in `src/`: any paragraph
            // in Spanish carries an accent somewhere.
            if line.chars().any(|c| "áéíóúñÁÉÍÓÚÑ¿¡".contains(c)) {
                offenders.push(format!("{}:{} {}", f.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} lines in Spanish:\n{}",
        offenders.len(),
        offenders.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn every_documented_mem_command_exists() {
    let help = String::from_utf8(
        std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
            .arg("--help")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    // The subcommands clap lists, one per indented line.
    let known: Vec<String> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    assert!(!known.is_empty(), "could not read the subcommands out of the help:\n{help}");

    let mut offenders = Vec::new();
    for f in living_docs() {
        let text = std::fs::read_to_string(&f).unwrap();
        let mut in_block = false;
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_block = line.trim_start().starts_with("```bash")
                    || line.trim_start().starts_with("```sh");
                continue;
            }
            if !in_block {
                continue;
            }
            let t = line.trim_start().trim_start_matches("$ ");
            let Some(rest) = t.strip_prefix("mem ") else { continue };
            // Skips the global flags to reach the subcommand. `--db` takes a
            // value, so its value has to be skipped as well: without that,
            // `mem --db bench.db init` reads `bench.db` as the subcommand and
            // the guard reports a command that nobody documented. It only
            // looked right while every documented path happened to carry a
            // `/`.
            let mut words = rest.split_whitespace();
            let mut sub = None;
            while let Some(w) = words.next() {
                if w == "--db" {
                    words.next();
                    continue;
                }
                if w.starts_with('-') || w.contains('/') || w.contains('~') {
                    continue;
                }
                sub = Some(w);
                break;
            }
            let Some(sub) = sub else { continue };
            if !known.iter().any(|k| k == sub) {
                offenders.push(format!("{}:{} `mem {sub}` does not exist", f.display(), n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "documented commands that do not exist:\n{}",
        offenders.join("\n")
    );
}

/// Every repository file a document names really exists.
///
/// The concrete reason: `scripts/install.sh` used to be `instalar.sh`, and
/// its URL — `raw/main/scripts/instalar.sh` — is the installation command the
/// README, `AGENTS.md` and `docs/installation.md` hand to people. Since it
/// points at `main`, renaming the file breaks that URL the instant the merge
/// lands, with nothing failing and nobody noticing until someone tries to
/// install.
///
/// It looks at repository paths, not at whole URLs: checking a URL would ask
/// the suite for network access, and what can break here is the rename, not
/// GitLab.
#[test]
fn every_repo_path_a_doc_mentions_exists() {
    // Paths like `scripts/x.sh`, `docs/y.md`, `src/z.rs`, `tools/w.py`,
    // whether loose, inside backticks or at the end of a URL.
    let paths_in = repo_paths_in_line();
    let mut offenders = Vec::new();
    for f in living_docs() {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            for path in &paths_in(line) {
                if !Path::new(path).exists() {
                    offenders.push(format!(
                        "{}:{} names {path}, which does not exist",
                        f.display(),
                        n + 1
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "documents naming files that do not exist:\n{}",
        offenders.join("\n")
    );
}

/// Pulls repository paths out of a line without depending on `regex`: this
/// test lives in the integration suite, and adding a dependency to recognise
/// four directories is not worth it.
fn repo_paths_in_line() -> impl Fn(&str) -> Vec<String> {
    |line: &str| {
        let mut out = Vec::new();
        for dir in ["scripts/", "docs/", "src/", "tools/", "tests/"] {
            let mut from = 0;
            while let Some(i) = line[from..].find(dir) {
                let start = from + i;
                let rest = &line[start..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || "`)\"',;".contains(c))
                    .unwrap_or(rest.len());
                let path = rest[..end].trim_end_matches('.').to_string();
                // Only paths with an extension: a mention of `docs/` with no
                // file names nothing that could be missing.
                if path.contains('.')
                    && !path.ends_with('/')
                    && !path.contains("historia/")
                    && !path.contains("superpowers/")
                {
                    out.push(path);
                }
                from = start + end.max(1);
            }
        }
        out
    }
}
