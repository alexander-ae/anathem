//! The export has to be able to come back: restoring a backup, moving the
//! memory to another machine, syncing two computers by hand with git.

use anathem::{db, import, ingest};
use std::path::Path;
use std::process::Command;

fn mem(db: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())
}

fn counts(p: &Path) -> (i64, i64, i64, i64, i64) {
    let c = rusqlite::Connection::open(p).unwrap();
    let q = |s: &str| c.query_row(s, [], |r| r.get::<_, i64>(0)).unwrap();
    (
        q("select count(*) from memories"),
        q("select count(*) from chunks"),
        q("select count(*) from memory_tags"),
        q("select count(*) from memory_refs"),
        q("select count(*) from memories where supersedes_id is not null"),
    )
}

#[test]
fn a_whole_database_survives_the_round_trip() {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("origen.db");
    {
        let conn = db::open(&source).unwrap();
        ingest::save_explicit(&conn, "primera nota con ticket ABC-1", "uno", vec!["alfa".into()])
            .unwrap();
        ingest::save_explicit(&conn, "segunda nota del proyecto dos", "dos", vec![]).unwrap();
        // A supersession chain, which is what forces ids to be remapped.
        ingest::save_with_topic(&conn, "version vieja del estado", "uno", vec![], Some("estado"), None)
            .unwrap();
        ingest::save_with_topic(&conn, "version nueva del estado", "uno", vec![], Some("estado"), None)
            .unwrap();
    }
    let exp = d.path().join("exp");
    let (ok, _, err) = mem(&source, &["export", "--out", exp.to_str().unwrap()]);
    assert!(ok, "export falló: {err}");

    let target = d.path().join("destino.db");
    let (ok, out, err) = mem(&target, &["import", "--desde", exp.to_str().unwrap()]);
    assert!(ok, "import falló: {err}");
    assert!(out.contains("importadas 4"), "salida: {out}");

    assert_eq!(counts(&source), counts(&target), "la base importada tiene que ser equivalente");

    // And it is searchable: the FTS5 index triggers ran.
    let (ok, out, _) = mem(&target, &["search", "segunda nota"]);
    assert!(ok);
    assert!(out.contains("segunda nota"), "la búsqueda no encuentra lo importado: {out}");
}

#[test]
fn importing_onto_a_nonempty_database_is_rejected_without_merge() {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("o.db");
    {
        let conn = db::open(&source).unwrap();
        ingest::save_explicit(&conn, "algo", "p", vec![]).unwrap();
    }
    let exp = d.path().join("exp");
    mem(&source, &["export", "--out", exp.to_str().unwrap()]);

    // The same database, which already has content.
    let (ok, _, err) = mem(&source, &["import", "--desde", exp.to_str().unwrap()]);
    assert!(!ok, "tenía que negarse: importar encima duplicaría todo");
    assert!(err.contains("--fusionar"), "el error tiene que decir la salida: {err}");

    // With --fusionar it does, and it does not duplicate what was there.
    let (ok, out, err) = mem(&source, &["import", "--desde", exp.to_str().unwrap(), "--fusionar"]);
    assert!(ok, "{err}");
    assert!(out.contains("importadas 0"), "no tenía que insertar nada: {out}");
    assert_eq!(counts(&source).0, 1, "seguía habiendo una sola memoria");
}

#[test]
fn a_file_without_frontmatter_does_not_break_the_import() {
    let d = tempfile::tempdir().unwrap();
    let exp = d.path().join("exp");
    std::fs::create_dir_all(exp.join("p")).unwrap();
    std::fs::write(exp.join("LEEME.md"), "# notas sueltas\n\nesto no es una memoria\n").unwrap();
    std::fs::write(
        exp.join("p").join("2026-01-01-una-memoria-1.md"),
        "---\nid: 1\nproject: \"p\"\ndescription: \"un titulo\"\ncreated: \"2026-01-01\"\n---\n\nel cuerpo\n",
    )
    .unwrap();
    let target = d.path().join("d.db");
    let (ok, out, err) = mem(&target, &["import", "--desde", exp.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("importadas 1"), "salida: {out}");
}

#[test]
fn the_fields_export_writes_are_read_back() {
    let txt = "---\nid: 7\nproject: \"api\"\ndescription: \"el titulo\"\ntags: [\"uno\", \"dos\"]\ncreated: \"2026-01-02\"\nupdated: \"2026-01-03\"\nstatus: \"superseded\"\nsource: \"migrated\"\npinned: true\ntopic: \"la-clave\"\next_id: \"ext-9\"\nsupersedes: 3\nevent_start: \"2026-01-01\"\nrefs:\n  - kind: \"ticket\"\n    value: \"ABC-1\"\n---\n\nel cuerpo de la memoria\n";
    let m = import::parse(txt).expect("tenía que leerse");
    assert_eq!(m.old_id, Some(7));
    assert_eq!(m.project, "api");
    assert_eq!(m.title, "el titulo");
    assert_eq!(m.tags, vec!["uno", "dos"]);
    assert_eq!(m.status, "superseded");
    assert_eq!(m.source, "migrated");
    assert!(m.pinned);
    assert_eq!(m.topic.as_deref(), Some("la-clave"));
    assert_eq!(m.ext_id.as_deref(), Some("ext-9"));
    assert_eq!(m.supersedes, Some(3));
    assert_eq!(m.event_start.as_deref(), Some("2026-01-01"));
    assert_eq!(m.refs, vec![("ticket".to_string(), "ABC-1".to_string())]);
    assert_eq!(m.body, "el cuerpo de la memoria");

    // With no frontmatter it is not a memory.
    assert!(import::parse("solo texto suelto").is_none());
    // Nor is it with frontmatter but no project.
    assert!(import::parse("---\nid: 1\n---\n\ncuerpo\n").is_none());
}
