//! `mem tidy` reports where a memory filed under `no-project` probably
//! belongs, and `mem project` is the command that acts on it.
//!
//! The split is the point: the report cannot be wrong in a way that matters,
//! because it changes nothing, and the move is deliberate. What is checked
//! here is that the suggestion is what it claims to be — a name that appears
//! in the memory, in the title before the body — and that the one invariant
//! the schema has about topic keys survives the move.

use anathem::{db, tidy};
use rusqlite::Connection;

fn projects() -> Vec<String> {
    ["anathem", "colegios.pe", "kipin.pe", "alexanderae.com", "infraestructura"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn a_name_in_the_title_decides_even_when_the_body_names_others() {
    let (p, why) = tidy::best_match(
        &projects(),
        "DROPLET DE DIGITALOCEAN: nada pendiente de migrar",
        "el blog alexanderae.com y la web colegios.pe apuntan al droplet",
    );
    assert_eq!(p, None, "the title names no project and the body names two: no answer is the answer");
    assert!(why.unwrap().contains('2'), "it has to say how many it found");

    let (p, why) = tidy::best_match(
        &projects(),
        "AUDITORÍA SEO DE colegios.pe",
        "también se revisó alexanderae.com de pasada",
    );
    assert_eq!(p.as_deref(), Some("colegios.pe"));
    assert!(why.unwrap().contains("title"));
}

#[test]
fn the_body_speaks_only_when_the_title_is_silent_and_there_is_one_candidate() {
    let (p, why) = tidy::best_match(&projects(), "Topes de abuso y CSP", "desplegado en colegios");
    assert_eq!(p.as_deref(), Some("colegios.pe"));
    assert!(why.unwrap().contains("body"));
}

/// `anathem` inside `anathemas` is not a mention of the project, and a stem
/// shorter than three characters would match half a corpus.
#[test]
fn a_mention_has_to_be_a_word() {
    let (p, _) = tidy::best_match(&projects(), "los anathemas del siglo xix", "");
    assert_eq!(p, None);
    let (p, _) = tidy::best_match(&projects(), "", "el proyecto Anathem existe");
    assert_eq!(p.as_deref(), Some("anathem"), "case does not matter");
}

fn db_with(rows: &[(&str, &str, Option<&str>)]) -> (tempfile::TempDir, Connection) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    for (project, title, topic) in rows {
        conn.execute(
            "insert into memories (project, title, body, created, updated, source, status, topic)
             values (?1, ?2, 'cuerpo', '2026-11-01', '2026-11-01', 'explicit', 'active', ?3)",
            rusqlite::params![project, title, topic],
        ).unwrap();
    }
    (d, conn)
}

#[test]
fn only_the_ones_with_no_project_are_listed() {
    let (_d, conn) = db_with(&[
        ("no-project", "una nota suelta sobre colegios.pe", None),
        ("colegios.pe", "otra", None),
    ]);
    let rows = tidy::candidates(&conn).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].suggestion.as_deref(), Some("colegios.pe"));
}

#[test]
fn a_move_changes_the_project_and_nothing_else() {
    let (_d, conn) = db_with(&[("no-project", "nota sobre kipin", None)]);
    let id: i64 = conn.query_row("select id from memories", [], |r| r.get(0)).unwrap();
    tidy::move_to(&conn, id, "kipin.pe").unwrap();
    let (project, body): (String, String) = conn
        .query_row("select project, body from memories where id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(project, "kipin.pe");
    assert_eq!(body, "cuerpo", "a project is a label, not part of what a memory says");
}

/// One active memory per (project, topic key) is the invariant topic keys
/// exist for. Moving a memory into a project that already holds that key
/// would break it, so the move stops and says why.
#[test]
fn a_move_that_would_duplicate_a_topic_key_is_refused() {
    let (_d, conn) = db_with(&[
        ("kipin.pe", "la versión buena", Some("deploy")),
        ("no-project", "la versión suelta", Some("deploy")),
    ]);
    let id: i64 = conn
        .query_row("select id from memories where project = 'no-project'", [], |r| r.get(0))
        .unwrap();
    let e = tidy::move_to(&conn, id, "kipin.pe").unwrap_err().to_string();
    assert!(e.contains("deploy") && e.contains("kipin.pe"), "{e}");
    let project: String = conn
        .query_row("select project from memories where id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(project, "no-project", "the refusal has to leave it where it was");
}

#[test]
fn moving_a_memory_that_does_not_exist_says_so() {
    let (_d, conn) = db_with(&[("no-project", "x", None)]);
    let e = tidy::move_to(&conn, 999, "kipin.pe").unwrap_err().to_string();
    assert!(e.contains("999"), "{e}");
}

// ------------------------------- keys: a slot, not a log

fn mem_row(conn: &Connection, project: &str, title: &str, topic: Option<&str>) -> i64 {
    conn.execute(
        "insert into memories (project, title, body, created, updated, source, status, topic)
         values (?1, ?2, 'cuerpo de la nota', '2026-11-01', '2026-11-01', 'explicit', 'active', ?3)",
        rusqlite::params![project, title, topic],
    ).unwrap();
    conn.last_insert_rowid()
}

/// A key is a slot and holds one active memory. Renaming one onto a key that
/// project already has in play would break that, so it stops and names the
/// memory in the way.
#[test]
fn a_key_already_in_play_is_refused_by_name() {
    let (_d, conn) = db_with(&[("p", "la que ya está", Some("deploy"))]);
    let id = mem_row(&conn, "p", "la nueva", None);
    let e = anathem::supersede::set_topic(&conn, id, "deploy", false).unwrap_err().to_string();
    assert!(e.contains("deploy"), "{e}");
    let topic: Option<String> = conn
        .query_row("select topic from memories where id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(topic, None, "the refusal has to leave it as it was");
}

#[test]
fn a_key_can_be_given_to_a_superseded_memory() {
    let (_d, conn) = db_with(&[("p", "la archivada", Some("bitacora"))]);
    let id: i64 = conn.query_row("select id from memories", [], |r| r.get(0)).unwrap();
    conn.execute("update memories set status = 'superseded' where id = ?1", [id]).unwrap();
    anathem::supersede::set_topic(&conn, id, "su-propio-tema", false).unwrap();
    let topic: String = conn
        .query_row("select topic from memories where id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(topic, "su-propio-tema");
}

/// Reviving withdraws the claim that replaced the memory, and it has to
/// withdraw it at both ends: the memory becomes active and the newer one
/// stops pointing at it. Leaving the pointer would say both things at once.
#[test]
fn reviving_withdraws_the_supersession_on_both_sides() {
    let (_d, conn) = db_with(&[("p", "la vieja", Some("log"))]);
    let old: i64 = conn.query_row("select id from memories", [], |r| r.get(0)).unwrap();
    conn.execute("update memories set status = 'superseded' where id = ?1", [old]).unwrap();
    let new = mem_row(&conn, "p", "la nueva", Some("log"));
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", [new, old]).unwrap();

    anathem::supersede::revive(&conn, old, Some("su-propio-tema")).unwrap();

    let (status, pointer): (String, Option<i64>) = conn
        .query_row("select status, supersedes_id from memories where id = ?1", [old], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(pointer, None);
    let still_points: Option<i64> = conn
        .query_row("select supersedes_id from memories where id = ?1", [new], |r| r.get(0))
        .unwrap();
    assert_eq!(still_points, None, "the claim that replaces it has to go with it");
}

#[test]
fn reviving_onto_a_key_in_play_asks_for_another() {
    let (_d, conn) = db_with(&[("p", "la vieja", Some("log"))]);
    let old: i64 = conn.query_row("select id from memories", [], |r| r.get(0)).unwrap();
    conn.execute("update memories set status = 'superseded' where id = ?1", [old]).unwrap();
    let new = mem_row(&conn, "p", "la nueva", Some("log"));
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", [new, old]).unwrap();

    let e = anathem::supersede::revive(&conn, old, None).unwrap_err().to_string();
    assert!(e.contains("--topic"), "{e}");
}

#[test]
fn only_a_superseded_memory_comes_back() {
    let (_d, conn) = db_with(&[("p", "una activa", None)]);
    let id: i64 = conn.query_row("select id from memories", [], |r| r.get(0)).unwrap();
    let e = anathem::supersede::revive(&conn, id, None).unwrap_err().to_string();
    assert!(e.contains("only a superseded"), "{e}");
}

/// The report this exists for: versions of a key that read as two subjects.
/// The threshold is a diagnostic, and the test pins the shape — one entry,
/// the worst pair first, and the count of loose ones.
#[test]
fn a_key_whose_versions_disagree_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let old = mem_row(&conn, "p", "tarjetas compactas y revisión de issues", Some("seguridad"));
    conn.execute(
        "update memories set body = 'tarjetas compactas de la bandeja y revisión de issues          abiertos en el repositorio', status = 'superseded' where id = ?1", [old]).unwrap();
    let new = mem_row(&conn, "p", "login con google configurado", Some("seguridad"));
    conn.execute(
        "update memories set body = 'login con google configurado para la web corporativa          con oauth', supersedes_id = ?3 where id = ?1 and ?2 is not null",
        rusqlite::params![new, rusqlite::types::Null, old]).unwrap();
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", [new, old]).unwrap();

    let rows = anathem::maint::loose_topics(&conn).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].topic, "seguridad");
    assert_eq!(rows[0].loose, 1);
    assert_eq!(rows[0].pairs[0].0, old);
    assert_eq!(rows[0].pairs[0].1, new);
}

#[test]
fn a_key_whose_versions_agree_is_silent() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let old = mem_row(&conn, "p", "despliegue en produccion con nginx y systemd", Some("deploy"));
    conn.execute(
        "update memories set body = 'despliegue en produccion con nginx y systemd funcionando',
                status = 'superseded' where id = ?1", [old]).unwrap();
    let new = mem_row(&conn, "p", "despliegue en produccion con nginx y systemd", Some("deploy"));
    conn.execute(
        "update memories set body = 'despliegue en produccion con nginx y systemd funcionando \
         otra vez' where id = ?1", [new]).unwrap();
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", [new, old]).unwrap();

    assert!(anathem::maint::loose_topics(&conn).unwrap().is_empty());
}
