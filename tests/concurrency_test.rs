//! What happens when there is more than one process.
//!
//! Every task in the plan protected the "never mix vectors" principle inside
//! its own process, and none of them had two processes in its diff. These
//! tests spawn real processes: one `mem` per terminal, as in real life.

mod support;

use std::path::Path;
use std::process::{Command, Stdio};

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

fn spawn(db: &Path, args: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(db)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Seeds vectorless chunks straight into the database, so that no `mem` has
/// synced them ahead of time. These are the "pending" ones that several
/// processes will then fight over at once.
fn seed_chunks_without_vectors(db: &Path, how_many: usize) {
    let conn = anathem::db::open(db).unwrap();
    conn.execute(
        "insert into memories (project, title, body, created, updated, source)
         values ('p', 'relleno', 'relleno', '2026-09-01', '2026-09-01', 'migrated')",
        [],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    // In a single transaction: seeding tens of thousands of chunks one
    // commit per row would take longer than the scenario being measured.
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
fn concurrent_saves_neither_fail_nor_lose_vectors() {
    // `sync` computed the pending set outside the transaction and inserted
    // it with a bare `insert` against a primary key: with several processes
    // at once, the ones that lost the race exited 1 with `UNIQUE constraint
    // failed` even though the memory had in fact been saved, and their
    // chunks were lost to the rollback. Over MCP the agent got `-32603
    // internal error` for a successful save.
    //
    // It also embedded INSIDE the write transaction, so the lock stayed
    // held for as long as the whole corpus took — and with no
    // `busy_timeout`, the other process failed immediately instead of
    // waiting its turn.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["model", "install", r.to_str().unwrap()]).status.success());

    // Enough chunks that the window between "see what is missing" and
    // "write it" is not instantaneous.
    seed_chunks_without_vectors(&db, 600);

    let children: Vec<_> = (0..6)
        .map(|i| spawn(&db, &["save", &format!("hola mundo guardado numero {i}")]))
        .collect();
    for (i, h) in children.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "el `mem save` {i} salió con error aunque la memoria se guarda igual: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    let conn = rusqlite::Connection::open(&db).unwrap();
    let saved_ones: i64 = conn
        .query_row("select count(*) from memories where source = 'explicit'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(saved_ones, 6, "se tienen que haber guardado las seis memorias");
    let sin_vector: i64 = conn
        .query_row(
            "select count(*) from chunks c
               left join chunk_vectors v on v.chunk_id = c.id
              where v.chunk_id is null",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sin_vector, 0, "quedaron {sin_vector} fragmentos sin vector");
}

#[test]
fn search_does_not_fail_even_when_it_cannot_reindex() {
    // The identity guard turned `mem search` into a writer that renamed the
    // file and reindexed the whole corpus, and propagated every error on
    // that path with `?`. With `packs/` read-only, searching exited 1 and
    // printed nothing, permanently — against the function's own docstring
    // and against the row of the spec saying search never fails because of
    // the vector layer.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola mundo"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());

    // The installed file becomes, in fact, a different pack: the guard
    // trips and the reindex needs to rename it.
    let packs = d.path().join("paquetes");
    support::write_pack_permuted(&packs.join("mini-es.anathem-pack"), "otro-es");

    // The rename has to fail, and it has to fail for anyone. This used to
    // be `packs/` in mode 0555, which does not stop root: in CI inside a
    // container — the usual case — the test skipped itself entirely and the
    // only deterministic scenario for "search does not fail even when it
    // cannot reindex" was covered by a race alone.
    //
    // A rename target occupied by a NON-EMPTY directory always fails,
    // without depending on permissions: `rename(2)` of a file onto a
    // directory returns EISDIR — and ENOTEMPTY, should the kernel look at
    // the contents first — for root as well. No uid can replace a populated
    // directory with a file; it is not a permission check.
    let target = packs.join("otro-es.anathem-pack");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("ocupado"), b"x").unwrap();

    let o = mem(&db, &["search", "hola"]);

    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(o.status.success(), "buscar no puede fallar por la capa vectorial: {err}");
    assert!(out.contains("hola"), "tiene que seguir devolviendo resultados de bm25: {out}");
    assert!(err.to_lowercase().contains("bm25"), "y decir que quedó degradada: {err}");
    // And that the scenario really was set up: what failed is the rename,
    // not anything else that would also degrade. Without this the test could
    // quietly become a no-op again — which is exactly how it used to void
    // itself when running as root.
    assert!(
        err.contains("renombrar"),
        "el escenario tiene que ser el rename imposible, no otro fallo: {err}"
    );

    // And nothing can have been left half done: the registry still names
    // the file that exists.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (name, _, _) = anathem::vecstore::registered_pack(&conn).unwrap().unwrap();
    assert!(
        packs.join(format!("{name}.anathem-pack")).exists(),
        "el registro quedó apuntando a un archivo que no existe: {name}"
    );
}

#[test]
fn two_parallel_searches_both_survive_with_the_guard_tripped() {
    // Both see the mismatch and both want to rename and reindex. The one
    // that lost the race used to die on the `rename`. Now it degrades to
    // bm25, which is a worse result but an honest one, and exits 0.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola mundo"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());
    support::write_pack_permuted(
        &d.path().join("paquetes").join("mini-es.anathem-pack"),
        "otro-es",
    );

    let children: Vec<_> = (0..2).map(|_| spawn(&db, &["search", "hola"])).collect();
    for (i, h) in children.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        assert!(
            o.status.success(),
            "la búsqueda {i} murió en la carrera por reindexar: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(out.contains("hola"), "la búsqueda {i} no devolvió nada: {out}");
    }
}

/// How many vectorless chunks the mixing scenario seeds.
///
/// It has to be enough that embedding them takes considerably longer than
/// starting a process and copying a fixture pack: at twenty thousand, the
/// `mem save` takes about a second and the `mem model install` lands in the
/// middle of it with close to a ten-to-one margin. It is the only parameter
/// of this test to raise if it ever turns flaky.
const BACKLOG: usize = 20_000;

#[test]
fn a_concurrent_install_leaves_no_vectors_from_two_packs() {
    // The hole: `sync` took a `&Pack` and never looked at the registry
    // again. A process that resolved pack A and then embedded a large
    // backlog kept writing A's vectors minutes later, even once another
    // terminal had run `mem model install B`: B's `adopt_pack` deleted the
    // A vectors already there, but did not stop the one still producing
    // them, and `on conflict do nothing` wrote them silently alongside B's.
    //
    // Both packs have the SAME dimensions on purpose: it is the real case —
    // the format fixes 256 for every pack — and the worst one, because
    // neither `search_vectors` nor `coverage` tells the blobs apart, and
    // since `sync` skips chunks that already have a vector, no later `mem
    // reindex` corrects it. The mixing is permanent.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    support::write_pack_mini(&a, "mini-es"); // ▁hola -> eje 0, ▁mundo -> eje 1
    support::write_pack_permuted(&b, "otro-es"); // ▁hola -> eje 3, ▁mundo -> eje 2
    assert!(mem(&db, &["init"]).status.success());
    // A is installed against an empty database, so nothing ends up indexed:
    // the backlog is seeded afterwards, which is the usual situation
    // (memories saved with no pack, or an `install` cut short with Ctrl-C).
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());
    seed_chunks_without_vectors(&db, BACKLOG);

    // Terminal 1: saves a memory, and on finishing drags the whole backlog
    // along with pack A.
    let saved_one = spawn(&db, &["save", "hola mundo"]);
    // Terminal 2: switches the pack while the other one embeds.
    std::thread::sleep(std::time::Duration::from_millis(100));
    let instalado = spawn(&db, &["model", "install", b.to_str().unwrap()]);

    let o = saved_one.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "el `mem save` tiene que salir con 0: la memoria se guardó igual. {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = instalado.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "el `mem model install` tiene que salir con 0: {}",
        String::from_utf8_lossy(&o.stderr)
    );

    let conn = rusqlite::Connection::open(&db).unwrap();
    let (name, _, dims) = anathem::vecstore::registered_pack(&conn).unwrap().unwrap();
    assert_eq!(name, "otro-es", "el último que adoptó fue el install de B");

    // No text in this test contains the piece "s", so under `otro-es` axes
    // 0 and 1 are exactly zero, and under `mini-es` they are precisely the
    // two that light up. A single vector with weight there is an A vector
    // living in an index that claims to be B's.
    let mut stmt = conn.prepare("select vec from chunk_vectors").unwrap();
    let vectors: Vec<Vec<f32>> = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|b| {
            b.unwrap().chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
        })
        .collect();
    assert!(!vectors.is_empty(), "el install de B tiene que haber dejado su índice");
    let intruders = vectors.iter().filter(|v| v[0] != 0.0 || v[1] != 0.0).count();
    assert_eq!(
        intruders,
        0,
        "{intruders} de {} vectores son del paquete mini-es en un índice registrado como \
         otro-es: la mezcla que no puede pasar nunca",
        vectors.len()
    );

    // And the acceptable outcome does have to have happened: the index left
    // behind is B's, complete, because nobody interrupted its `sync`.
    let (covered, tot) = anathem::vecstore::coverage(&conn, dims).unwrap();
    assert_eq!(covered, tot, "el paquete que ganó tiene que terminar su propio índice");
}
