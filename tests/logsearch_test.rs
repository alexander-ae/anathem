use anathem::{db, logsearch};
use rusqlite::params;

fn conn_vacia() -> (tempfile::TempDir, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    (dir, conn)
}

#[test]
fn mark_follow_up_no_matchea_por_substring() {
    let (_d, conn) = conn_vacia();
    // returned_ids contiene "131" y "132" pero nunca el id "1" como entrada propia.
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values ('2026-09-22T00:00:00', 'q', null, ?1, 0)",
        params!["131,101,132"],
    ).unwrap();

    logsearch::mark_follow_up(&conn, 1).unwrap();

    let followed_up: i64 = conn
        .query_row("select followed_up from search_log where id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(followed_up, 0, "id=1 no debería matchear vía substring dentro de 131/132");
}

#[test]
fn mark_follow_up_matchea_id_genuino_en_la_lista() {
    let (_d, conn) = conn_vacia();
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values ('2026-09-22T00:00:00', 'q', null, ?1, 0)",
        params!["131,101,1,132"],
    ).unwrap();

    logsearch::mark_follow_up(&conn, 1).unwrap();

    let followed_up: i64 = conn
        .query_row("select followed_up from search_log where id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(followed_up, 1, "id=1 está genuinamente en la lista y debe marcarse");
}
