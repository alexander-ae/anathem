mod support;

use anathem::{db, ingest, model::NewMemory, pack::Pack, search, vecstore};
use rusqlite::Connection;

fn seed(conn: &Connection, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap()
}

fn fixture() -> (tempfile::TempDir, Connection, Pack) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    (dir, conn, p)
}

#[test]
fn sync_fills_the_gaps_and_is_idempotent() {
    let (_d, conn, p) = fixture();
    seed(&conn, "hola");
    seed(&conn, "mundo");
    assert_eq!(support::register_and_sync(&conn, &p).written(), 2);
    assert_eq!(support::register_and_sync(&conn, &p).written(), 0, "no debe reembeber");
    assert_eq!(vecstore::coverage(&conn, p.dims()).unwrap(), (2, 2));
}

#[test]
fn coverage_shows_the_gap_left_by_saves_with_no_pack() {
    let (_d, conn, p) = fixture();
    seed(&conn, "hola");
    support::register_and_sync(&conn, &p);
    seed(&conn, "mundo"); // guardada después, sin sync
    assert_eq!(vecstore::coverage(&conn, p.dims()).unwrap(), (1, 2));
}

#[test]
fn search_orders_by_cosine() {
    let (_d, conn, p) = fixture();
    let hola = seed(&conn, "hola");
    let mundo = seed(&conn, "mundo");
    support::register_and_sync(&conn, &p);
    let q = search::Query { text: String::new(), ..Default::default() };
    let v = p.embed("hola").unwrap();
    let r = vecstore::search_vectors(&conn, &v, &q, 10).unwrap();
    assert_eq!(r[0].memory_id, hola, "la más parecida debe ir primera");
    assert!(r[0].cosine > r[1].cosine);
    assert!(r.iter().any(|a| a.memory_id == mundo));
}

#[test]
fn search_respects_the_project_filter() {
    let (_d, conn, p) = fixture();
    let a = ingest::insert(&conn, &NewMemory {
        project: "uno".into(), title: "t".into(), body: "hola".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "dos".into(), title: "t".into(), body: "hola".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    support::register_and_sync(&conn, &p);
    let q = search::Query { project: Some("uno".into()), ..Default::default() };
    let r = vecstore::search_vectors(&conn, &p.embed("hola").unwrap(), &q, 10).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].memory_id, a);
}

#[test]
fn search_ignores_memories_that_are_not_active() {
    let (_d, conn, p) = fixture();
    let id = seed(&conn, "hola");
    support::register_and_sync(&conn, &p);
    conn.execute("update memories set status='forgotten' where id=?1", [id]).unwrap();
    let r = vecstore::search_vectors(&conn, &p.embed("hola").unwrap(), &search::Query::default(), 10).unwrap();
    assert!(r.is_empty(), "una memoria olvidada no puede salir en la búsqueda vectorial");
}

#[test]
fn the_pack_registry_round_trips() {
    let (_d, conn, _p) = fixture();
    assert!(vecstore::registered_pack(&conn).unwrap().is_none());
    vecstore::register_pack(&conn, "en-es", "abc123", 256).unwrap();
    assert_eq!(
        vecstore::registered_pack(&conn).unwrap(),
        Some(("en-es".into(), "abc123".into(), 256))
    );
    // registering again overwrites, it does not duplicate
    vecstore::register_pack(&conn, "en-es-pt", "def456", 256).unwrap();
    assert_eq!(
        vecstore::registered_pack(&conn).unwrap(),
        Some(("en-es-pt".into(), "def456".into(), 256))
    );
}

#[test]
fn forgetting_the_pack_clears_registry_and_vectors() {
    let (_d, conn, p) = fixture();
    seed(&conn, "hola");
    support::register_and_sync(&conn, &p);
    vecstore::register_pack(&conn, "mini-es", "abc", 4).unwrap();
    vecstore::forget_pack(&conn).unwrap();
    assert!(vecstore::registered_pack(&conn).unwrap().is_none());
    assert_eq!(vecstore::coverage(&conn, p.dims()).unwrap(), (0, 1));
}

// Companion to the test below, for Windows: `#[cfg(unix)]` would take the
// test out of the build entirely, and there it would not even show as
// "ignored" — only a lower total that nobody is going to compare against
// Linux's. With this version, `cargo test` on Windows counts it and marks it
// ignored with the reason in plain sight, instead of making it disappear.
#[cfg(windows)]
#[test]
#[ignore = "el cuerpo real usa PermissionsExt y un $EDITOR de shell (shebang), \
            ninguno de los dos existe en Windows; ver la versión unix de \
            este test, un poco más abajo en este archivo"]
fn editing_a_memory_invalidates_its_vectors() {
    unimplemented!(
        "esta prueba no puede correr en Windows: depende de PermissionsExt \
         y de un $EDITOR de mentira que es un script de shell"
    )
}

// Skipped on Windows: the fake `$EDITOR` is a shell script this test marks
// executable by hand with `PermissionsExt`, which does not exist outside
// Unix. What it proves — that `mem edit` syncs the new vectors — depends on
// nothing platform-specific; it is the way of faking the editor that does.
// There is no portable way to build a one-line `$EDITOR` without invoking a
// shell interpreter. The Windows version of this test, above, puts on record
// that it is skipped and why, instead of vanishing from the build.
#[cfg(unix)]
#[test]
fn editing_a_memory_invalidates_its_vectors() {
    // Re-chunking deletes the chunks and the cascade takes the vectors with
    // it. That half always worked; the missing one was somebody rebuilding
    // the new ones.
    //
    // The previous version of this test called `sync` by hand after
    // `replace_body`, so it stayed green while `mem edit` — the one write
    // path that did not sync — left the edited memory without a single
    // vector. It gave false confidence about precisely the path that was
    // broken. Now it runs the real binary.
    let (dir, _conn, p) = fixture();
    let db = dir.path().join("t.db");
    let out_text = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).args(["save", "hola"]).output().unwrap();
    assert!(out_text.status.success());

    // Installing the pack leaves the memory with its vector for "hola".
    let out_text = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db)
        .args(["model", "install", dir.path().join("mini.anathem-pack").to_str().unwrap()])
        .output().unwrap();
    assert!(out_text.status.success(), "{}", String::from_utf8_lossy(&out_text.stderr));

    // A fake $EDITOR that replaces the body with different text.
    let editor = dir.path().join("editor.sh");
    std::fs::write(&editor, "#!/bin/sh\nprintf 'mundo' > \"$1\"\n").unwrap();
    std::fs::set_permissions(
        &editor,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let id: i64 = {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.query_row("select id from memories", [], |r| r.get(0)).unwrap()
    };
    let out_text = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).args(["edit", &id.to_string()])
        .env("EDITOR", &editor).output().unwrap();
    assert!(out_text.status.success(), "{}", String::from_utf8_lossy(&out_text.stderr));

    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        vecstore::coverage(&conn, p.dims()).unwrap(),
        (1, 1),
        "después de `mem edit` la memoria tiene que quedar con su vector new_id, \
         no sin vectores hasta el próximo `mem reindex`"
    );
    // And the vector has to be the NEW text's: "mundo" is axis 1.
    let blob: Vec<u8> = conn
        .query_row("select vec from chunk_vectors", [], |r| r.get(0))
        .unwrap();
    let v: Vec<f32> = blob.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    assert!(v[1] > 0.9, "el vector debe ser el de \"mundo\" (eje 1), salió {v:?}");
}

#[test]
fn search_collapses_several_chunks_of_one_memory() {
    // `seed` uses bodies of 4-5 characters, which always fit in a single
    // chunk (CHUNK_SIZE=500) and never exercise the collapse-by-maximum in
    // `search_vectors` (the `best.entry(mid).or_insert(...)`). This memory is
    // deliberately long so as to force at least two chunks: the first,
    // padding of "mundo", and the second, the only one containing "hola" — so
    // the result depends on `search_vectors` picking the closest chunk and
    // not either of the two.
    let (_d, conn, p) = fixture();
    let relleno = "mundo ".repeat(100); // 600 caracteres, puro "mundo"
    let goal = "hola ".repeat(50); // 250 caracteres, puro "hola"
    let body = format!("{relleno}{goal}");
    let id = seed(&conn, &body);

    let n_fragmentos: i64 = conn
        .query_row("select count(*) from chunks where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert!(
        n_fragmentos >= 2,
        "la memoria debe partirse en al menos dos fragmentos para que este test sirva de algo; \
         si el chunking cambia y ahora produce {n_fragmentos}, hay que ajustar el cuerpo de prueba"
    );

    support::register_and_sync(&conn, &p);

    // By construction, "hola" appears in only one chunk: the first 500
    // characters fall entirely inside the "mundo" padding.
    let expected_id: i64 = conn
        .query_row(
            "select id from chunks where memory_id = ?1 and body like '%hola%'",
            [id],
            |r| r.get(0),
        )
        .unwrap();

    let q = search::Query::default();
    let r = vecstore::search_vectors(&conn, &p.embed("hola").unwrap(), &q, 10).unwrap();
    let hits_: Vec<&vecstore::VectorHit> = r.iter().filter(|a| a.memory_id == id).collect();
    assert_eq!(hits_.len(), 1, "la memoria debe aparecer una sola vez, no una por fragmento");
    assert_eq!(
        hits_[0].chunk_id, expected_id,
        "debe quedarse con el fragmento más parecido, no con cualquiera"
    );
    assert!(hits_[0].cosine > 0.0, "el fragmento elegido debe tener coseno positivo con \"hola\"");
}

#[test]
fn registered_pack_propagates_real_errors_instead_of_swallowing_them() {
    // `.ok()` on `query_row` would turn any real sqlite error (missing
    // table, corrupt database) into `None`, indistinguishable from "no pack
    // registered": search would silently degrade to bm25 and nobody would
    // learn the database is broken. We force a real error (the `meta` table
    // is missing) and confirm it propagates instead of hiding.
    let (_d, conn, _p) = fixture();
    conn.execute("drop table meta", []).unwrap();
    let err = vecstore::registered_pack(&conn).unwrap_err();
    assert!(
        err.to_string().contains("no such table"),
        "un error real de sqlite no debe convertirse en None; error visto: {err}"
    );
}

/// Puts `how_many` vectorless chunks into a filler memory, in a single
/// transaction. They are the "backlog" that takes a while to embed and during
/// which the pack can change underneath.
fn seed_pending(conn: &Connection, how_many: usize) {
    let id = seed(conn, "relleno");
    conn.execute("delete from chunks where memory_id = ?1", [id]).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    for i in 0..how_many {
        tx.execute(
            "insert into chunks (memory_id, ord, body) values (?1, ?2, ?3)",
            rusqlite::params![id, i as i64, format!("hola mundo numero {i}")],
        )
        .unwrap();
    }
    tx.commit().unwrap();
}

#[test]
fn sync_writes_not_one_vector_under_a_stale_identity() {
    // The minimal form of the defect: the caller resolved pack A and, by the
    // time it finished embedding, the database is already registered with B.
    // Without the check inside the write transaction, A's vectors went into
    // an index that claims to be B's, and `on conflict do nothing` did not
    // even raise an error. With equal dimensions — the real case — nothing
    // tells them apart afterwards.
    let (_d, conn, p) = fixture();
    seed(&conn, "hola");
    let stale = ("mini-es".to_string(), "sha-de-a".to_string(), p.dims());
    vecstore::register_pack(&conn, "otro-es", "sha-de-b", p.dims()).unwrap();

    let r = vecstore::sync(&conn, &p, &stale).unwrap();

    assert_eq!(
        r,
        vecstore::SyncOutcome::Abandoned(0),
        "con la identidad caducada el lote se abandona, no se escribe"
    );
    assert_eq!(
        vecstore::coverage(&conn, p.dims()).unwrap(),
        (0, 1),
        "no puede haber entrado un solo vector del paquete que ya no está registrado"
    );
}

#[test]
fn sync_abandons_midway_when_the_pack_changes_between_batches() {
    // The case that actually happens: the backlog is large, the first batch
    // is written with the right pack and halfway through another terminal
    // runs `mem model install`. The following batches kept being written
    // with the old pack alongside the new one's, and since `sync` skips
    // chunks that already have a vector, no later `mem reindex` corrected
    // it: the mixing stayed forever.
    //
    // The "other process" change is made by a sqlite trigger on
    // `chunk_vectors`, which is the only way for the exact moment to be the
    // same on every run: it fires inside the first batch's transaction, just
    // like someone else's `adopt_pack` winning the lock right there.
    let (_d, conn, p) = fixture();
    let total = 300usize; // más de un lote (el lote son 256)
    seed_pending(&conn, total);
    let identity = ("mini-es".to_string(), "sha-de-a".to_string(), p.dims());
    vecstore::register_pack(&conn, &identity.0, &identity.1, identity.2).unwrap();
    conn.execute_batch(
        "create trigger cambia_el_paquete after insert on chunk_vectors
         begin
           update meta set value = 'otro-es' where key = 'pack_nombre';
         end;",
    )
    .unwrap();

    let r = vecstore::sync(&conn, &p, &identity).unwrap();

    assert!(r.abandoned(), "el cambio de paquete a mitad de camino tiene que abandonar: {r:?}");
    let written: i64 =
        conn.query_row("select count(*) from chunk_vectors", [], |r| r.get(0)).unwrap();
    assert_eq!(written as usize, r.written(), "lo reportado y lo escrito tienen que coincidir");
    assert!(written > 0, "el primer lote sí se escribió con la identidad correcta");
    assert!(
        (written as usize) < total,
        "se escribieron los {total} fragmentos con un paquete que ya no era el registrado"
    );
}

/// The project and tag filters have to hold on the vector side exactly as on
/// the lexical one. They used to be written twice; this test pins down that
/// the vector does not return what bm25 discards, which is what that
/// duplication was going to cost the day the two sides drifted apart.
#[test]
fn the_vector_side_respects_the_same_filters_as_the_lexical_one() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let conn = anathem::db::open(&db).unwrap();

    anathem::ingest::save_explicit(&conn, "nota del proyecto uno", "uno", vec!["alfa".into()])
        .unwrap();
    anathem::ingest::save_explicit(&conn, "nota del proyecto dos", "dos", vec!["beta".into()])
        .unwrap();

    // Identical vectors for all of them: that way the cosine breaks no ties
    // and the only thing that can leave something out is the filter.
    let dims = 4usize;
    let v: Vec<u8> = [1.0f32, 0.0, 0.0, 0.0].iter().flat_map(|x| x.to_le_bytes()).collect();
    for id in conn
        .prepare("select id from chunks")
        .unwrap()
        .query_map([], |r| r.get::<_, i64>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>()
    {
        conn.execute("insert into chunk_vectors (chunk_id, vec) values (?1, ?2)", rusqlite::params![id, v])
            .unwrap();
    }
    let query_ = vec![1.0f32, 0.0, 0.0, 0.0];
    assert_eq!(query_.len(), dims);

    let with_project = anathem::search::Query {
        text: "nota".into(),
        project: Some("uno".into()),
        tags: vec![],
        limit: 10,
        budget_tokens: 10_000,
        excerpt: None,
    };
    let r = anathem::vecstore::search_vectors(&conn, &query_, &with_project, 50).unwrap();
    assert_eq!(r.len(), 1, "el filtro de proyecto tiene que valer también acá: {r:?}");

    let with_tag = anathem::search::Query {
        text: "nota".into(),
        project: None,
        tags: vec!["beta".into()],
        limit: 10,
        budget_tokens: 10_000,
        excerpt: None,
    };
    let r = anathem::vecstore::search_vectors(&conn, &query_, &with_tag, 50).unwrap();
    assert_eq!(r.len(), 1, "el filtro de tags tiene que valer también acá: {r:?}");

    // And with no filters, both of them.
    let sin = anathem::search::Query {
        text: "nota".into(),
        project: None,
        tags: vec![],
        limit: 10,
        budget_tokens: 10_000,
        excerpt: None,
    };
    let r = anathem::vecstore::search_vectors(&conn, &query_, &sin, 50).unwrap();
    assert_eq!(r.len(), 2, "sin filtros tienen que venir las dos: {r:?}");
}
