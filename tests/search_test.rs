use anathem::{db, ingest, model::NewMemory, search};
use rusqlite::Connection;

fn seed(conn: &Connection, ext: &str, project: &str, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: project.into(),
        title: body.chars().take(60).collect(),
        body: body.into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

fn fixture() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "a", "acme-billing", "El cache del breakdown no se justifica porque agrega staleness sin ganancia medible.");
    seed(&conn, "b", "acme-billing", "Convenciones de testing: la suite de integración cubre el status 500 con manejo de excepciones.");
    seed(&conn, "c", "docs-portal", "El design system interno define los colores de la plataforma admin.");
    (dir, conn)
}

#[test]
fn it_finds_by_term() {
    let (_d, conn) = fixture();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert!(!hits.is_empty(), "sin resultados");
    assert_eq!(hits[0].ext_id.as_deref(), Some("a"));
}

#[test]
fn it_filters_by_project() {
    let (_d, conn) = fixture();
    let q = search::Query {
        text: "design system".into(),
        project: Some("acme-billing".into()),
        ..Default::default()
    };
    assert!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().is_empty(), "no debe cruzar de proyecto cuando se filtra");
}

#[test]
fn it_deduplicates_by_memory() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    // a long body => several chunks, all with the same word
    let largo = "cache ".repeat(400);
    seed(&conn, "x", "p", &largo);
    let q = search::Query { text: "cache".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(hits.len(), 1, "una memoria debe aparecer una sola vez, hubo {}", hits.len());
}

#[test]
fn it_excludes_superseded_ones() {
    let (_d, conn) = fixture();
    conn.execute("update memories set status='superseded' where ext_id='a'", []).unwrap();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert!(hits.iter().all(|h| h.ext_id.as_deref() != Some("a")));
}

#[test]
fn it_respects_the_token_budget() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..20 {
        seed(&conn, &format!("m{i}"), "p", &format!("cache numero {i} {}", "x".repeat(2000)));
    }
    let q = search::Query { text: "cache".into(), budget_tokens: 300, ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    let tokens: usize = hits.iter().map(|h| (h.title.len() + h.snippet.len()) / 4).sum();
    assert!(tokens <= 300, "se pasó del presupuesto: {tokens} tokens");
    assert!(!hits.is_empty(), "el presupuesto no debe vaciar el resultado");
}

#[test]
fn pinned_ones_come_first() {
    let (_d, conn) = fixture();
    conn.execute("update memories set pinned=1 where ext_id='b'", []).unwrap();
    let q = search::Query {
        text: "cache testing".into(),
        project: Some("acme-billing".into()),
        ..Default::default()
    };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("b"), "la pinned debe ir primero");
}

#[test]
fn the_order_is_deterministic_on_bm25_ties() {
    // Regression for finding C: `best` was a HashMap and `into_values()`
    // guarantees no order; with no final tiebreak after the score, two
    // memories tied on bm25 could come out in a different order between
    // runs. We seed many memories with an identical body (the exact same
    // bm25) to force the tie and compare the order returned across repeated
    // runs.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..40 {
        seed(&conn, &format!("tie{i}"), "p", "faro identico para forzar empate de bm25");
    }
    let q = search::Query { text: "faro".into(), project: Some("p".into()), limit: 40, ..Default::default() };

    let primera: Vec<Option<String>> = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().into_iter().map(|h| h.ext_id).collect();
    for _ in 0..5 {
        let siguiente: Vec<Option<String>> = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().into_iter().map(|h| h.ext_id).collect();
        assert_eq!(primera, siguiente, "el orden cambió entre corridas idénticas");
    }
}

#[test]
fn pinned_survives_the_candidate_trim() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();

    // 210 memories matching the term strongly: more than enough to fill the
    // 200-candidate quota the SQL query builds with "order by score limit
    // 200".
    for i in 0..210 {
        seed(&conn, &format!("c{i}"), "p", &"faro ".repeat(30));
    }

    // A pinned memory matching weakly (the term appears once, buried in
    // padding): under pure bm25 it falls below the 200-candidate cut.
    let relleno = format!("{}faro", "relleno ".repeat(50));
    seed(&conn, "pinned1", "p", &relleno);
    conn.execute("update memories set pinned = 1 where ext_id = 'pinned1'", []).unwrap();

    let q = search::Query {
        text: "faro".into(),
        project: Some("p".into()),
        limit: 5,
        ..Default::default()
    };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(
        hits[0].ext_id.as_deref(),
        Some("pinned1"),
        "la pinned debe sobrevivir aunque su bm25 caiga fuera del top 200 de candidatos"
    );
}

mod support;

#[test]
fn with_no_pack_the_result_is_the_usual_one() {
    let (_d, conn) = fixture();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("a"));
}

#[test]
fn with_a_pack_what_bm25_misses_shows_up() {
    // The mini fixture only knows "hola", "mundo" and "s". A memory whose
    // body is "mundo" does not match the query "hola" under bm25, but it is
    // close by vector, so the fusion has to bring it to the surface.
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::open_pack(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    seed(&conn, "y", "p", "mundo");
    support::register_and_sync(&conn, &p);

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let solo = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(solo.len(), 1, "bm25 solo encuentra la que comparte la palabra");

    let ctx = search::SearchContext::for_reading(Some(&p), 0.25);
    let covered = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(covered.len(), 2, "la fusión debe traer también la vectorial");
    assert_eq!(covered[0].ext_id.as_deref(), Some("x"), "la exacta sigue primera");
}

#[test]
fn pinned_memories_still_come_first_under_fusion() {
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::open_pack(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    let pinned_one = seed(&conn, "y", "p", "mundo");
    anathem::maint::pin(&conn, pinned_one, true).unwrap();
    support::register_and_sync(&conn, &p);

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let ctx = search::SearchContext::for_reading(Some(&p), 0.25);
    let hits = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("y"), "la fijada va primera igual");
}

#[test]
fn a_query_with_no_known_tokens_yields_no_spurious_results() {
    // Fix round 1: the mini tokenizer only knows "hola" and "mundo";
    // everything else falls to <unk>, whose row is the null vector. Without
    // the zero-norm check, the cosine of the null vector against anything is
    // 0 and `vecstore::search_vectors` would return arbitrary memories all
    // tied at 0: garbage wearing the look of a valid result.
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::open_pack(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    seed(&conn, "y", "p", "mundo");
    support::register_and_sync(&conn, &p);

    let q = search::Query { text: "xyz".into(), ..Default::default() };
    let ctx = search::SearchContext::for_reading(Some(&p), 0.25);
    let hits = search::run(&conn, &q, &ctx).unwrap();
    assert!(
        hits.is_empty(),
        "una consulta sin tokens conocidos no debe devolver resultados espurios, dio {}",
        hits.len()
    );
}

/// Twelve memories with the same long body (which fits in a single chunk,
/// see `ingest::CHUNK_SIZE`), so we can compare how many fit in the same
/// token budget with and without excerpt trimming.
fn fixture_cuerpos_largos() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..12 {
        let body = format!("faro {i} {}", "x".repeat(465));
        seed(&conn, &format!("faro{i}"), "p", &body);
    }
    (dir, conn)
}

#[test]
fn the_excerpt_lets_more_candidates_fit_in_the_same_budget() {
    // This is the test proving the trim happens BEFORE the cost counts
    // against the budget, inside `run_explained`, and not after: were it
    // trimmed at the end (in `mcp.rs`, say), the budget would already have
    // cut the list down to the size the full chunks gave, and returning more
    // short candidates would change nothing.
    let (_d, conn) = fixture_cuerpos_largos();
    let ctx = search::SearchContext::bm25_only();

    let sin_recorte = search::Query {
        text: "faro".into(),
        limit: 12,
        budget_tokens: 300,
        excerpt: None,
        ..Default::default()
    };
    let with_trim = search::Query { excerpt: Some(20), ..sin_recorte.clone() };

    let hits_sin = search::run_explained(&conn, &sin_recorte, &ctx).unwrap();
    let hits_con = search::run_explained(&conn, &with_trim, &ctx).unwrap();

    assert!(
        hits_con.len() > hits_sin.len(),
        "con excerpt recortado tienen que entrar más candidates en el mismo presupuesto: \
         sin recorte {} candidates, con recorte {}",
        hits_sin.len(),
        hits_con.len()
    );
    assert_eq!(hits_con.len(), 12, "con extractos de 20 caracteres las 12 memorias caben en el presupuesto");
}

#[test]
fn the_excerpt_trims_the_chunk_to_n_characters() {
    let (_d, conn) = fixture_cuerpos_largos();
    let ctx = search::SearchContext::bm25_only();
    let q = search::Query {
        text: "faro".into(),
        limit: 12,
        budget_tokens: usize::MAX,
        excerpt: Some(20),
        ..Default::default()
    };
    let hits = search::run_explained(&conn, &q, &ctx).unwrap();
    assert!(!hits.is_empty(), "sin resultados");
    for e in &hits {
        assert_eq!(
            e.hit.snippet.chars().count(),
            20,
            "el fragmento debe quedar recortado a 20 caracteres, salió {:?}",
            e.hit.snippet
        );
    }
}

#[test]
fn the_excerpt_does_not_split_a_multibyte_character() {
    // The corpus is in Spanish and carries accents: trimming by bytes would
    // cut a UTF-8 character in half (splitting "ñ" or "á" into two bytes that
    // no longer form a valid character). A Rust `String` cannot hold invalid
    // bytes, so an incorrect trim would not produce an odd string: it would
    // panic on construction. This test trims right in the middle of a run of
    // accents so as to force that case.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    // Each padding character takes more than one byte in UTF-8 (2 bytes
    // each), so cutting by bytes at an odd index would split one. The space
    // between repetitions is needed for FTS5 to tokenize them as separate
    // words rather than one 300-character word.
    let body = "áéíóúñ ".repeat(50);
    seed(&conn, "acentos", "p", &body);

    for n in [1usize, 3, 5, 7, 41] {
        let q = search::Query {
            text: "áéíóúñ".into(),
            limit: 1,
            budget_tokens: usize::MAX,
            excerpt: Some(n),
            ..Default::default()
        };
        let hits = search::run_explained(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
        assert_eq!(hits.len(), 1, "n={n}: se esperaba 1 resultado");
        assert_eq!(
            hits[0].hit.snippet.chars().count(),
            n,
            "n={n}: el fragmento debe tener exactamente n caracteres"
        );
    }
}

#[test]
fn excerpt_none_leaves_the_whole_chunk_as_before() {
    let (_d, conn) = fixture();
    let q_viejo = search::Query { text: "cache staleness".into(), ..Default::default() };
    let q_explicito = search::Query { text: "cache staleness".into(), excerpt: None, ..Default::default() };
    let ctx = search::SearchContext::bm25_only();

    let hits_viejo = search::run_explained(&conn, &q_viejo, &ctx).unwrap();
    let hits_explicito = search::run_explained(&conn, &q_explicito, &ctx).unwrap();

    assert_eq!(hits_viejo.len(), hits_explicito.len());
    for (a, b) in hits_viejo.iter().zip(hits_explicito.iter()) {
        assert_eq!(a.hit.snippet, b.hit.snippet, "extracto: None no debe alterar el fragmento");
    }
    // And the chunk is still the whole body, not a trimmed one.
    assert!(
        hits_viejo[0].hit.snippet.chars().count() > 20,
        "sin extracto el fragmento debe seguir siendo el completo"
    );
}

// ---------------------------------------------------------------------------
// `--scope key:value`: narrowing the pool to a project, a tag or a topic.
// The SQL for project and tags was already in `filters_for`; what was
// missing was a way to say it, and a way to compose it.
// ---------------------------------------------------------------------------

fn mem(dir: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("t.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn scoped(raw: &[&str]) -> anathem::search::Scopes {
    anathem::search::parse_scopes(&raw.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn scopes_parse_into_the_filters_that_already_existed() {
    let s = scoped(&["project:acme-billing", "tag:decision", "tag:perf"]);
    assert_eq!(s.project.as_deref(), Some("acme-billing"));
    assert_eq!(s.tags, vec!["decision".to_string(), "perf".to_string()]);
    assert_eq!(s.topic, None);
}

#[test]
fn an_unknown_scope_key_is_an_error_and_not_a_filter_dropped_in_silence() {
    // A filter dropped quietly returns too much, and whoever wrote it never
    // finds out.
    let e = anathem::search::parse_scopes(&["autor:alexander".to_string()]).unwrap_err();
    let msg = e.to_string();
    assert!(msg.contains("autor"), "tiene que nombrar la clave que no entiende: {msg}");
    assert!(msg.contains("project") && msg.contains("tag") && msg.contains("topic"),
        "y cuáles acepta: {msg}");
}

#[test]
fn a_scope_without_a_colon_or_without_a_value_is_an_error() {
    for malo in ["project", "project:", ":decision", "   "] {
        assert!(
            anathem::search::parse_scopes(&[malo.to_string()]).is_err(),
            "«{malo}» no es un scope y no puede pasar en silencio"
        );
    }
}

#[test]
fn two_project_scopes_cannot_both_be_satisfied_so_they_are_an_error() {
    let e = anathem::search::parse_scopes(&[
        "project:a".to_string(),
        "project:b".to_string(),
    ])
    .unwrap_err();
    assert!(e.to_string().contains("project"), "{e}");
}

#[test]
fn two_scopes_narrow_with_and() {
    let (_d, conn) = fixture();
    let con_tag = seed(&conn, "d", "acme-billing", "Decidimos cachear el breakdown igual, con TTL corto.");
    conn.execute("insert into memory_tags (memory_id, tag) values (?1, 'decision')", [con_tag])
        .unwrap();

    let s = scoped(&["project:acme-billing", "tag:decision"]);
    let q = search::Query { text: "cache breakdown".into(), ..s.onto(search::Query::default()).unwrap() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(hits.len(), 1, "solo la que cumple las dos: {hits:?}");
    assert_eq!(hits[0].ext_id.as_deref(), Some("d"));
}

#[test]
fn a_topic_scope_narrows_to_that_subject() {
    let (_d, conn) = fixture();
    let con_topic =
        seed(&conn, "e", "acme-billing", "El proveedor de pagos es Adyen desde octubre.");
    conn.execute("update memories set topic = 'payment-provider' where id = ?1", [con_topic])
        .unwrap();
    seed(&conn, "f", "acme-billing", "El proveedor de correo es Postmark.");

    let s = scoped(&["topic:payment-provider"]);
    let q = search::Query { text: "proveedor".into(), ..s.onto(search::Query::default()).unwrap() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].ext_id.as_deref(), Some("e"));
}

#[test]
fn a_scope_that_matches_nothing_says_so_instead_of_no_results() {
    // "No results for this query" and "that scope holds nothing" are
    // different answers, and only one of them means rephrasing would help.
    let (d, _conn) = fixture();
    let (ok, out, _) = mem(d.path(), &["search", "cache", "--scope", "tag:no-existe"]);
    assert!(ok);
    assert!(
        out.contains("tag:no-existe"),
        "tiene que decir qué scope quedó vacío, no «sin resultados»: {out}"
    );
}

#[test]
fn project_given_twice_and_differently_is_refused() {
    let (d, _conn) = fixture();
    let (ok, _out, err) =
        mem(d.path(), &["search", "cache", "--project", "a", "--scope", "project:b"]);
    assert!(!ok, "dos proyectos distintos no pueden cumplirse a la vez");
    assert!(err.contains("project"), "{err}");
}

#[test]
fn the_cli_and_the_mcp_filter_identically_through_the_same_parser() {
    use anathem::{config::Config, mcp};
    let (_d, conn) = fixture();
    let con_tag = seed(&conn, "d", "acme-billing", "Decidimos cachear el breakdown igual.");
    conn.execute("insert into memory_tags (memory_id, tag) values (?1, 'decision')", [con_tag])
        .unwrap();

    let s = scoped(&["project:acme-billing", "tag:decision"]);
    let q = search::Query {
        text: "cache breakdown".into(),
        limit: 20,
        ..s.onto(search::Query::default()).unwrap()
    };
    let por_biblioteca: Vec<i64> = search::run(&conn, &q, &search::SearchContext::bm25_only())
        .unwrap()
        .iter()
        .map(|h| h.memory_id)
        .collect();

    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{
            "query":"cache breakdown",
            "scope":["project:acme-billing","tag:decision"]}}});
    let r = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let text = r["result"]["content"][0]["text"].as_str().unwrap().to_string();
    let por_mcp: Vec<i64> = text
        .lines()
        .filter_map(|l| l.strip_prefix("[id:"))
        .filter_map(|l| l.split(']').next())
        .filter_map(|n| n.parse().ok())
        .collect();

    assert_eq!(por_biblioteca, por_mcp, "both surfaces have to filter the same: {text}");
}

#[test]
fn an_invalid_scope_over_mcp_is_an_explained_error_and_not_a_crash() {
    use anathem::{config::Config, mcp};
    let (_d, conn) = fixture();
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"cache","scope":["autor:x"]}}});
    let r = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let text = serde_json::to_string(&r).unwrap();
    assert!(text.contains("autor"), "the model has to be able to correct it: {text}");
}

// ---------------------------------------------------------------------------
// The deterministic outline: what a long memory looks like from outside,
// computed from its own text, with no model anywhere near it.
// ---------------------------------------------------------------------------

const LARGA: &str = "El runbook de la caché cubre el ciclo entero de la invalidación.\n\n\
# Síntomas\n\nLa latencia sube en el percentil 99 y el breakdown queda viejo.\n\n\
# Diagnóstico\n\nSe mira el TTL efectivo contra el que dice la configuración.\n\n\
# Remedio\n\nSe purga por prefijo y se vuelve a calentar con el job nocturno.\n\n\
# Lo que no hay que hacer\n\nPurgar entero en hora punta: la estampida tira la base.\n\n\
Relleno para que la memoria pase el umbral y el esquema valga la pena: \
una nota corta no necesita índice y el índice no puede gastar tokens repitiendo \
lo que el fragmento ya dice. Este párrafo existe nada más para que el cuerpo sea \
largo de verdad y la prueba mida lo que dice medir, no un caso de borde. El umbral está por encima del tamaño de un fragmento a propósito: una memoria que entra entera en un fragmento no tiene un resto sobre el que el lector pueda dudar, así que no hay nada que resumirle. Este relleno la empuja por encima de ese umbral sin inventar secciones que no existen, que es lo que la prueba necesita para medir el caso real y no uno de borde.";

#[test]
fn a_long_memory_shows_its_headings_in_the_outline() {
    let o = anathem::search::outline(LARGA, 120).expect("esta memoria sí tiene qué indexar");
    assert!(o.contains("Síntomas") && o.contains("Remedio"), "{o}");
    assert!(
        o.contains("runbook"),
        "y el primer párrafo, que es de qué trata: {o}"
    );
    assert!(
        !o.contains("estampida"),
        "pero no el cuerpo entero, que es lo que get_memory devuelve: {o}"
    );
}

#[test]
fn a_memory_with_no_headings_gets_no_outline_and_keeps_its_excerpt() {
    // Measured over LongMemEval-S: 98.3% of memories clear the threshold and
    // 1.5% of those carry a heading. Handing back the first paragraph for
    // the rest would have replaced the text that matched the query with the
    // memory's opening lines — same tokens, less to go on.
    let plana = "Una sola idea larga y sin títulos. ".repeat(40);
    assert!(plana.chars().count() > anathem::search::OUTLINE_THRESHOLD);
    assert_eq!(anathem::search::outline(&plana, 120), None);
}

#[test]
fn a_memory_below_the_threshold_gets_no_outline_either() {
    let corta = "La caché del breakdown no se justifica.";
    assert_eq!(anathem::search::outline(corta, 120), None);
}

#[test]
fn the_outline_is_off_unless_it_is_asked_for() {
    let (_d, conn) = fixture();
    let id = seed(&conn, "larga", "acme-billing", LARGA);
    assert!(id > 0);

    let q = search::Query { text: "runbook caché".into(), ..Default::default() };
    let sin = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert!(!sin.is_empty());
    // The chunk that matched may well contain some headings — they are in
    // its first 500 characters. What it cannot contain is the index of the
    // WHOLE memory, which is the thing the outline adds.
    assert!(
        !sin[0].snippet.contains("sections:"),
        "sin pedirlo, el fragmento tiene que ser el de siempre: {}",
        sin[0].snippet
    );

    let q = search::Query {
        text: "runbook caché".into(),
        outline: true,
        excerpt: Some(200),
        ..Default::default()
    };
    let con = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert!(
        con[0].snippet.contains("sections:"),
        "pidiéndolo, tiene que verse la estructura: {}",
        con[0].snippet
    );
    assert!(
        con[0].snippet.contains("Lo que no hay que hacer"),
        "incluidas las secciones que caen fuera del fragmento que ganó: {}",
        con[0].snippet
    );
}
