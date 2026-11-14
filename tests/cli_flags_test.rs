//! The flags moved to English (task 3). The Spanish forms stay as hidden
//! aliases: anything already written in a script keeps working, and the help
//! shows one name per flag.

use std::path::Path;

fn mem(db: &Path, args: &[&str]) -> (String, bool) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).to_string()
            + &String::from_utf8_lossy(&o.stderr),
        o.status.success(),
    )
}

fn base(d: &Path) -> std::path::PathBuf {
    let db = d.join("m.db");
    assert!(mem(&db, &["init"]).1);
    db
}

#[test]
fn the_new_english_flags_work() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--since", "6h", "--limit", "3"]).1);
    assert!(mem(&db, &["recent", "--all"]).1);
}

#[test]
fn the_old_spanish_flags_still_work() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--desde", "6h", "--limite", "3"]).1);
    assert!(mem(&db, &["recent", "--todos"]).1);
}

#[test]
fn import_takes_from_and_merge_in_english() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    let vacia = d.path().join("vacia");
    std::fs::create_dir(&vacia).unwrap();
    assert!(mem(&db, &["import", "--from", vacia.to_str().unwrap(), "--merge"]).1);
    // And the old form still works.
    assert!(mem(&db, &["import", "--desde", vacia.to_str().unwrap(), "--fusionar"]).1);
}

#[test]
fn the_old_flags_are_hidden_from_help() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["recent", "--help"]);
    assert!(ok, "{help}");
    assert!(help.contains("--since"), "the new one has to appear: {help}");
    assert!(help.contains("--limit"), "the new one has to appear: {help}");
    assert!(!help.contains("--desde"), "the old one cannot appear: {help}");
    assert!(!help.contains("--limite"), "the old one cannot appear: {help}");
}

#[test]
fn the_help_is_english_and_well_formed() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["--help"]);
    assert!(ok, "{help}");
    // The first line of each doc comment is the subcommand's summary: if one
    // is cut badly during a translation it disappears from the help.
    for sub in ["save", "search", "recent", "doctor", "export", "import", "model", "serve"] {
        assert!(help.contains(sub), "missing subcommand {sub}: {help}");
    }
    for word in ["memoria", "búsqueda", "proyecto", "Guarda", "archivo", "paquete"] {
        assert!(!help.contains(word), "Spanish left in the help: «{word}»\n{help}");
    }
}

#[test]
fn the_unbounded_window_keyword_is_the_one_the_help_advertises() {
    // `--help` said `--since all` while the code only accepted `todo`: the
    // help taught a value the binary rejects. Introduced while translating
    // the doc comment without translating the sentinel it compares.
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--since", "all"]).1, "`all` is what the help advertises");
    // And the old form is still accepted, as with the flags.
    assert!(mem(&db, &["recent", "--since", "todo"]).1, "`todo` used to work and is not broken now");
}

/// Somebody can hold this binary without ever having seen the repository —
/// an agent ran the installer for them. `--help` is where they look to find
/// out what it is, so the URL is there; `doctor` is where they look when
/// something is wrong, so it is there too.
///
/// Taken from `Cargo.toml` rather than written out, so it cannot drift from
/// what crates.io and the release publish.
#[test]
fn the_help_and_doctor_say_where_the_project_lives() {
    let url = env!("CARGO_PKG_REPOSITORY");
    assert!(url.starts_with("https://"), "the repository has to be a URL: {url}");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--help")
        .output()
        .unwrap();
    let ayuda = String::from_utf8_lossy(&output.stdout);
    assert!(ayuda.contains(url), "`mem --help` does not say where the project lives:\n{ayuda}");

    let d = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(d.path().join("mem.db"))
        .arg("doctor")
        .output()
        .unwrap();
    let doctor = String::from_utf8_lossy(&output.stdout);
    assert!(doctor.contains(url), "`mem doctor` does not say it either:\n{doctor}");
}

/// And `--version` does not, because its output is parsed: `mem update`
/// compares against it and the installer reads it. A URL there buys no
/// discoverability that `--help` does not already give, and costs a parser.
#[test]
fn the_version_stays_machine_readable() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--version")
        .output()
        .unwrap();
    let v = String::from_utf8_lossy(&output.stdout);
    assert!(!v.contains("http"), "--version has to stay one line with no URL: {v}");
    assert!(v.starts_with("mem "), "{v}");
}
