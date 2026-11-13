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
    let casa = dir.path().join("anathem");
    let db = casa.join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "una nota con algo privado adentro"]));

    assert_eq!(mode(&db), 0o600, "la base tiene que ser 0600");
    assert_eq!(mode(&casa), 0o700, "el directorio tiene que ser 0700");
    for sufijo in ["-wal", "-shm"] {
        let p = casa.join(format!("mem.db{sufijo}"));
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
    let casa = dir.path().join("anathem");
    let db = casa.join("mem.db");
    assert!(mem(&db, &["init"]));

    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&casa, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(mem(&db, &["search", "lo que sea"]));
    assert_eq!(mode(&db), 0o600, "abrir una base vieja tiene que restringirla");
    assert_eq!(mode(&casa), 0o700, "y también su directorio");
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

    let instalado = anathem::packstore::packs_dir(dir.path()).join("mini-es.anathem-pack");
    assert_eq!(mode(&instalado), 0o400, "el paquete instalado tiene que ser 0400");

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
            std::fs::OpenOptions::new().write(true).truncate(true).open(&instalado).is_err(),
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
    assert_eq!(mode(&instalado), 0o400);
}

/// `mem doctor` tiene que decir cuando la base está abierta a otros usuarios.
///
/// El diagnóstico abre la base en `immutable=1`, así que NO pasa por
/// `db::open` y por lo tanto no aplica la corrección de permisos. Eso está
/// bien —su contrato dice que no modifica nada— pero callarlo no: una
/// instalación con la base en 0644 recibía dieciocho líneas en verde y un
/// «All good.» sin una palabra sobre el asunto. Medido sobre una instalación
/// real que venía de una versión anterior.
#[test]
fn doctor_reports_a_world_readable_database() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("anathem");
    let db = home.join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "una nota"]));

    // Como la dejaría una versión anterior al arreglo.
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();

    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).arg("doctor").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();

    assert!(out.contains("0644"), "tiene que decir qué permisos tiene: {out}");
    assert!(
        out.to_lowercase().contains("readable") || out.to_lowercase().contains("other users"),
        "y qué significa: {out}"
    );
    // Es un aviso y no un problema: la base funciona perfectamente.
    assert_eq!(o.status.code(), Some(0), "no puede hacer fallar el diagnóstico: {out}");

    // Y no la corrige: doctor no modifica nada.
    let modo = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
    assert_eq!(modo, 0o644, "doctor no tiene que cambiar los permisos");
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

/// The export is the copy that leaves the machine; it used to be 0644.
#[test]
fn an_export_leaves_nothing_readable_by_others() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "a note with something private inside", "--project", "api"]));

    let out = dir.path().join("export");
    assert!(mem(&db, &["export", "--out", out.to_str().unwrap()]));

    assert_eq!(mode(&out), 0o700, "the directory the export creates is 0700");
    let project = out.join("api");
    assert_eq!(mode(&project), 0o700, "the project directory carries titles in its file names");
    let file = std::fs::read_dir(&project)
        .unwrap()
        .next()
        .expect("one exported memory")
        .unwrap()
        .path();
    assert_eq!(mode(&file), 0o600, "every exported memory is 0600: {}", file.display());
}

/// An existing directory keeps its mode: it belongs to whoever made it.
#[test]
fn an_export_into_a_directory_that_already_existed_does_not_change_its_mode() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mem.db");
    assert!(mem(&db, &["init"]));
    assert!(mem(&db, &["save", "another private note", "--project", "api"]));

    let out = dir.path().join("already-there");
    std::fs::create_dir(&out).unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(mem(&db, &["export", "--out", out.to_str().unwrap()]));

    assert_eq!(mode(&out), 0o755, "an existing directory is left alone");
    assert_eq!(mode(&out.join("api")), 0o700, "its contents are not");
}
