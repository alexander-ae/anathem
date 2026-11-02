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
