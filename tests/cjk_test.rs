//! Chinese and Japanese have no spaces inside a sentence, so `unicode61`
//! indexes an entire run as one token and a search for anything inside it
//! matches nothing. The fix is to index CJK runs as overlapping character
//! bigrams, the way Lucene's CJKBigramFilter does, and to put the query
//! through the same transformation.

use anathem::chunk::segment_cjk;

#[test]
fn latin_text_is_left_exactly_as_it_was() {
    // The published figures are all non-CJK. If this changed anything about
    // Latin text, every one of them would move.
    for s in ["el proveedor de pagos es Adyen", "a-b_c 42 ¿qué?", ""] {
        assert_eq!(segment_cjk(s), s, "it must not touch {s:?}");
    }
}

#[test]
fn a_run_of_cjk_becomes_overlapping_bigrams() {
    assert_eq!(segment_cjk("設定ファイル"), "設定 定フ ファ ァイ イル");
}

#[test]
fn a_single_cjk_character_survives_on_its_own() {
    // There is no bigram to make, and dropping it would lose the only token
    // that memory has.
    assert_eq!(segment_cjk("田"), "田");
}

#[test]
fn cjk_mixed_with_latin_keeps_both_and_the_boundary() {
    assert_eq!(segment_cjk("pagos 設定 ok"), "pagos 設定 ok");
    assert_eq!(segment_cjk("決済はAdyenです"), "決済 済は Adyen です");
}

#[test]
fn hangul_is_not_touched_because_korean_is_written_with_spaces() {
    // Korean already works. Bigramming it would break what works.
    assert_eq!(segment_cjk("결제 제공자는 Adyen 입니다"), "결제 제공자는 Adyen 입니다");
}

#[test]
fn punctuation_ends_a_run() {
    // The ideographic full stop is a separator for unicode61 already, so a
    // bigram must not straddle it and invent an adjacency.
    assert_eq!(segment_cjk("決済です。設定です"), "決済 済で です 。 設定 定で です");
}

use anathem::{db, ingest, model::NewMemory, search};

fn save(conn: &rusqlite::Connection, body: &str) {
    ingest::insert(conn, &NewMemory {
        project: "p".into(), title: body.chars().take(8).collect(), body: body.into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
}

fn find(conn: &rusqlite::Connection, query: &str) -> usize {
    let q = search::Query { text: query.into(), ..Default::default() };
    search::run(conn, &q, &search::SearchContext::bm25_only()).unwrap().len()
}

#[test]
fn a_japanese_memory_is_found_by_a_substring_of_its_sentence() {
    // The bug: `設定ファイル` is literally inside the body, and before the
    // bigrams the whole clause was one token, so nothing matched.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    save(&conn, "決済プロバイダーはAdyenです。設定ファイルに書いてあります。");
    assert_eq!(find(&conn, "設定ファイル"), 1, "a substring has to find it");
}

#[test]
fn a_chinese_memory_is_found_by_a_substring_of_its_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    save(&conn, "数据库连接池的最大连接数是一百");
    assert_eq!(find(&conn, "数据库"), 1);
}

fn top(conn: &rusqlite::Connection, query: &str) -> String {
    let q = search::Query { text: query.into(), ..Default::default() };
    search::run(conn, &q, &search::SearchContext::bm25_only()).unwrap()
        .first().map(|h| h.snippet.clone()).unwrap_or_default()
}

#[test]
fn latin_search_still_ranks_the_way_it_did() {
    // The guard on everything this project has published. It checks the
    // ranking and not the count on purpose: `to_match` joins terms with OR,
    // so a shared word like "de" legitimately returns both memories, and a
    // count would be asserting something search never promised.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    save(&conn, "El proveedor de pagos es Adyen desde octubre");
    save(&conn, "Las ramas salen de main y vuelven por merge request");
    assert!(top(&conn, "proveedor de pagos").contains("Adyen"), "got {:?}", top(&conn, "proveedor de pagos"));
    assert!(top(&conn, "ramas main merge").contains("ramas"), "got {:?}", top(&conn, "ramas main merge"));
    assert_eq!(find(&conn, "zzzz inexistente"), 0);
}

#[test]
fn an_existing_database_is_migrated_and_its_cjk_becomes_searchable() {
    // The case that matters for anybody who already uses this: the memory
    // was saved before the bigrams existed, so it is sitting in the index as
    // one token. Opening the database has to fix it, without being asked.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let conn = db::open(&path).unwrap();
        save(&conn, "決済プロバイダーはAdyenです。設定ファイルに書いてあります。");
        save(&conn, "El proveedor de pagos es Adyen desde octubre");

        // Put it back the way a database written before this change looked:
        // the FTS table over `body`, the old triggers, no `fts_body`.
        conn.execute_batch(
            r#"drop trigger if exists chunks_ai;
               drop trigger if exists chunks_ad;
               drop trigger if exists chunks_au;
               drop table if exists chunks_fts;
               alter table chunks drop column fts_body;
               create virtual table chunks_fts using fts5(
                 body, content='chunks', content_rowid='id',
                 tokenize="unicode61 remove_diacritics 2");
               create trigger chunks_ai after insert on chunks begin
                 insert into chunks_fts(rowid, body) values (new.id, new.body);
               end;
               insert into chunks_fts(chunks_fts) values ('rebuild');"#,
        ).unwrap();
    }

    // Before reopening, the old shape really is broken: this is the bug.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let n: i64 = conn.query_row(
            "select count(*) from chunks_fts where chunks_fts match '\"設定ファイル\"'",
            [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "the fixture has to reproduce the bug, or this proves nothing");
    }

    let conn = db::open(&path).unwrap();
    assert_eq!(find(&conn, "設定ファイル"), 1, "opening it has to repair the index");
    assert!(top(&conn, "proveedor de pagos").contains("Adyen"), "and not disturb the Latin memory");
}
