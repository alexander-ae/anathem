//! B-1 and M-2: what anathem writes stays readable only by its owner.

#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;

extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}
use std::path::Path;

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn mem(db: &Path, args: &[&str]) -> bool {
    std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args)
        .output().unwrap().status.success()
}

#[test]
fn the_database_and_its_directory_are_private() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("anathem");
    let db = home.join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "una nota con algo privado adentro"]));

    assert_eq!(mode(&db), 0o600, "la base tiene que ser 0600");
    assert_eq!(mode(&home), 0o700, "el directorio tiene que ser 0700");
    for sufijo in ["-wal", "-shm"] {
        let p = home.join(format!("mem.db{sufijo}"));
        if p.exists() {
            assert_eq!(mode(&p), 0o600, "{sufijo} tiene el mismo contenido que la base");
        }
    }
}

#[test]
fn an_existing_database_left_world_readable_gets_narrowed() {
    // The case that matters: not a fresh install, but the one that has been
    // accumulating notes under an earlier version that created it 0644.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("anathem");
    let db = home.join("mem.db");
    assert!(mem(&db, &["init"]));

    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(mem(&db, &["search", "lo que sea"]));
    assert_eq!(mode(&db), 0o600, "abrir una base vieja tiene que restringirla");
    assert_eq!(mode(&home), 0o700, "y también su directorio");
}

#[test]
fn mem_edit_leaves_nothing_behind_in_the_shared_temp_directory() {
    // M-2: the temporary used to be `/tmp/anathem-edit-{id}.md` — fixed
    // name, world-readable, and left behind when the editor failed. Now it
    // lives in a private directory that goes away with it.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "cuerpo original"]));

    let viejo = std::env::temp_dir().join("anathem-edit-1.md");
    let habia = viejo.exists();

    // An editor that fails: the path that used to skip the cleanup.
    let ok = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).args(["edit", "1"])
        .env("EDITOR", "false")
        .output().unwrap().status.success();
    assert!(!ok, "un editor que falla tiene que hacer fallar el comando");
    assert_eq!(viejo.exists(), habia, "no puede quedar nada en la ruta predecible de antes");
}

/// B-2: an installed pack cannot be written over in place. It stays mapped
/// for the life of the MCP server, and truncating a mapped file is SIGBUS.
#[test]
fn an_installed_pack_is_read_only_and_still_replaceable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "hola mundo"]));

    let origen = dir.path().join("mini.anathem-pack");
    support::write_pack_mini(&origen, "mini-es");
    assert!(mem(&db, &["model", "install", origen.to_str().unwrap()]));

    let installed = anathem::packstore::packs_dir(dir.path()).join("mini-es.anathem-pack");
    assert_eq!(mode(&installed), 0o400, "the installed pack has to be 0400");

    // Writing over it in place is what the mode prevents — for everyone
    // except root, who ignores permission bits entirely. CI runs inside a
    // container as root, so asserting it there would fail on a protection
    // that is working exactly as designed.
    //
    // The repository already learned this once: `concurrency_test` says a
    // `packs/` in mode 0555 "does not stop root: in CI inside a container —
    // the usual case — the test skipped itself entirely". That one found a
    // mechanism no uid can bypass. Here there is none: 0400 against root is
    // simply not a thing, so the honest move is to assert it where it
    // holds and say so where it does not.
    if unsafe { libc_geteuid() } == 0 {
        eprintln!("saltado: root ignora los bits de permiso, el 0400 no lo detiene");
    } else {
        assert!(
            std::fs::OpenOptions::new().write(true).truncate(true).open(&installed).is_err(),
            "no tendría que poder truncarse en su lugar"
        );
    }

    // And none of that blocks a reinstall, which renames over it.
    let otro = dir.path().join("otro.anathem-pack");
    support::write_pack_permuted(&otro, "mini-es");
    assert!(
        mem(&db, &["model", "install", otro.to_str().unwrap()]),
        "reinstalar tiene que seguir funcionando: el rename pide permiso sobre el directorio"
    );
    assert_eq!(mode(&installed), 0o400);
}

/// `mem doctor` has to say when the database is open to other users.
///
/// The diagnosis opens the database with `immutable=1`, so it does NOT go
/// through `db::open` and therefore does not apply the permission fix. That
/// is right — its contract says it changes nothing — but staying silent is
/// not: an installation with the database at 0644 got eighteen green lines
/// and an "All good." without a word about it. Measured on a real
/// installation that came from an earlier version.
#[test]
fn doctor_reports_a_world_readable_database() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("anathem");
    let db = home.join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "una nota"]));

    // As a version before the fix would leave it.
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();

    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).arg("doctor").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();

    assert!(out.contains("0644"), "it has to say which mode it has: {out}");
    assert!(
        out.to_lowercase().contains("readable") || out.to_lowercase().contains("other users"),
        "and what it means: {out}"
    );
    // A notice, not a problem: the database works.
    assert_eq!(o.status.code(), Some(0), "it cannot fail the diagnosis: {out}");

    // And it does not correct it: doctor changes nothing.
    let current = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
    assert_eq!(current, 0o644, "doctor must not change the mode");
}

#[test]
fn doctor_says_nothing_when_the_permissions_are_right() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("anathem").join("mem.db");
    assert!(mem(&db, &["init"]));
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).arg("doctor").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(!out.contains("0644"), "con 0600 no hay nada que decir: {out}");
}
