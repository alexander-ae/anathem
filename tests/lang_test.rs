//! `Lang` and the shared producers that honour it.
//!
//! The CLI and the MCP server share the functions that turn data into text.
//! These tests pin both sides at once: the Spanish the terminal has always
//! printed, and the English the model now gets.

use anathem::lang::Lang;
use anathem::recent::duration_in_seconds;

#[test]
fn a_bad_window_explains_itself_in_the_asked_language() {
    let es = duration_in_seconds("ayer", Lang::Es).unwrap_err().to_string();
    assert!(es.contains("ventana"), "el español de hoy se conserva: {es}");
    let en = duration_in_seconds("ayer", Lang::En).unwrap_err().to_string();
    assert!(en.to_lowercase().contains("window"), "en inglés: {en}");
    assert!(!en.contains("ventana"), "no puede quedar español: {en}");
}

#[test]
fn a_good_window_parses_the_same_in_both() {
    assert_eq!(duration_in_seconds("6h", Lang::En).unwrap(), 21_600);
    assert_eq!(duration_in_seconds("6h", Lang::Es).unwrap(), 21_600);
}

#[test]
fn an_empty_window_and_a_non_positive_one_also_switch() {
    for (entrada, _) in [("", "vacía"), ("0h", "positiva")] {
        let es = duration_in_seconds(entrada, Lang::Es).unwrap_err().to_string();
        let en = duration_in_seconds(entrada, Lang::En).unwrap_err().to_string();
        assert_ne!(es, en, "los tres errores tienen que tener versión en inglés: {entrada:?}");
    }
}

// --- describe() y format_items ------------------------------------------

#[test]
fn a_save_that_replaced_nothing_does_not_claim_it_did() {
    // `describe()` tiene cuatro ramas y esta es la que más fácil se traduce
    // al revés: clave nueva, nada reemplazado.
    let d = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&d.path().join("m.db")).unwrap();
    let g = anathem::ingest::save_with_topic(
        &conn, "una nota", "p", vec![], Some("clave-nueva"), None).unwrap();

    let en = g.describe(Lang::En).unwrap();
    assert!(!en.to_lowercase().contains("replac"), "no reemplazó nada: {en}");
    assert!(en.contains("clave-nueva"), "tiene que nombrar la clave: {en}");

    let es = g.describe(Lang::Es).unwrap();
    assert!(es.contains("no había una memoria activa"), "el español de hoy: {es}");
}

#[test]
fn a_save_that_did_replace_says_so_in_both() {
    let d = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&d.path().join("m.db")).unwrap();
    anathem::ingest::save_with_topic(
        &conn, "primera", "p", vec![], Some("tema"), None).unwrap();
    let g = anathem::ingest::save_with_topic(
        &conn, "segunda", "p", vec![], Some("tema"), None).unwrap();

    let en = g.describe(Lang::En).unwrap();
    assert!(en.to_lowercase().contains("replac"), "sí reemplazó: {en}");
    let es = g.describe(Lang::Es).unwrap();
    assert!(es.contains("reemplaza"), "{es}");
}

#[test]
fn format_items_switches_language_but_not_content() {
    let d = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&d.path().join("m.db")).unwrap();
    anathem::ingest::save_explicit(&conn, "el cuerpo de la nota", "p", vec![]).unwrap();
    let items = anathem::recent::list_recent(&conn, Some("p"), None, 5, 120).unwrap();

    // El contenido del usuario no se traduce en ninguno de los dos.
    for lang in [Lang::En, Lang::Es] {
        let t = anathem::recent::format_items(&items, false, lang);
        assert!(t.contains("el cuerpo de la nota"), "{lang:?}: {t}");
    }
}

#[test]
fn an_empty_recent_says_so_in_each_language() {
    let es = anathem::recent::format_items(&[], false, Lang::Es);
    assert!(es.contains("sin memorias"), "{es}");
    let en = anathem::recent::format_items(&[], false, Lang::En);
    assert!(en.to_lowercase().contains("no memories"), "{en}");
}
