mod support;

use anathem::pack::Pack;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn it_opens_and_reports_its_metadata() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    assert_eq!(p.name(), "mini-es");
    assert_eq!(p.dims(), 4);
}

#[test]
fn a_single_token_yields_its_axis() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
    assert!(v[1].abs() < 1e-5 && v[2].abs() < 1e-5 && v[3].abs() < 1e-5);
}

#[test]
fn two_tokens_average_their_rows() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("hola mundo").unwrap();
    let expected = 1.0f32 / 2f32.sqrt();
    assert!((v[0] - expected).abs() < 1e-5, "salió {v:?}");
    assert!((v[1] - expected).abs() < 1e-5, "salió {v:?}");
}

#[test]
fn the_suffix_is_re_segmented_instead_of_falling_back_to_unknown() {
    // "mundos" is not a piece, but "▁mundo" and "s" are. This is the
    // property that lets the vocabulary be trimmed without losing quality.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("mundos").unwrap();
    assert!(v[1] > 0.1 && v[2] > 0.1, "esperaba mezcla de ▁mundo y s, salió {v:?}");
}

#[test]
fn empty_text_yields_a_null_vector_without_failing() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("").unwrap();
    assert_eq!(v.len(), 4);
    assert!(cos(&v, &v) < 1e-9, "esperaba el vector nulo, salió {v:?}");
}

#[test]
fn it_rejects_a_file_without_the_magic() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("basura.anathem-pack");
    std::fs::write(&r, b"esto no es un paquete en absoluto").unwrap();
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("ANPK"), "el error debe nombrar la marca: {e}");
}

#[test]
fn it_rejects_a_version_it_does_not_understand() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    support::corrupt_byte(&r, 4, 99); // el primer byte de la versión
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("version"), "el error debe nombrar la versión: {e}");
}

#[test]
fn it_rejects_a_truncated_file() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let b = std::fs::read(&r).unwrap();
    std::fs::write(&r, &b[..b.len() - 5]).unwrap();
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("truncated"), "el error debe nombrar la causa: {e}");
    assert!(e.contains("mini.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn it_rejects_a_corrupt_tokenizer_length_without_panicking() {
    // "tokenizer length" is a u64 read raw from the file. If a corrupt pack
    // declares it near u64::MAX, adding it to the current offset must not
    // overflow the pointer arithmetic: under overflow-checks (`cargo test`'s
    // default) that would panic with "attempt to add with overflow" instead
    // of returning an Err. We corrupt the field's 8 bytes to force that
    // value.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let base = 20 + "mini-es".len(); // desplazamiento del campo tok_len (u64 LE)
    for i in 0..8 {
        support::corrupt_byte(&r, base + i, 0xFF);
    }
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("tokenizer"), "el error debe nombrar la causa: {e}");
    assert!(e.contains("mini.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn the_sha256_is_stable_and_changes_with_the_content() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    let a = anathem::pack::sha256(&r).unwrap();
    assert_eq!(a, anathem::pack::sha256(&r).unwrap());
    assert_eq!(a.len(), 64);
    support::corrupt_byte(&r, 40, 7);
    assert_ne!(a, anathem::pack::sha256(&r).unwrap());
}

#[test]
fn it_rejects_a_table_smaller_than_the_vocabulary() {
    // A pack with fewer rows than its tokenizer has pieces opens cleanly,
    // installs, reports full coverage and produces NOTHING but null vectors:
    // `embed` skips the rows that do not exist, and when all are missing it
    // skips all of them. Not one sign that the pack is good for nothing.
    // `embed`'s `continue` is the right net against panicking; the
    // inconsistency is caught on open, once, and naming the file.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("corto.anathem-pack");
    support::write_pack_rows(&r, "corto-es", 4, &[vec![0, 0, 0, 0], vec![10, 0, 0, 0]]);
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("tokenizer"), "el error debe explicar la incoherencia: {e}");
    assert!(e.contains("corto.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn it_accepts_an_added_token_whose_id_is_outside_the_table() {
    // The table covers exactly the model's four pieces, which is what the
    // generator writes. The tokenizer also carries an `added_token` with id
    // 7, outside that range — the shape of any pruned sentencepiece.
    //
    // The check measured `get_vocab_size(true)`, the UNION of the model's
    // vocabulary with the `added_tokens`: it gave 5 against 4 rows and
    // rejected a healthy pack. The right count is the model vocabulary's;
    // added ids falling outside the table are skipped by `embed`.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("agregados.anathem-pack");
    support::write_pack_with_tokenizer(
        &r,
        "agregados-es",
        4,
        &[vec![0, 0, 0, 0], vec![10, 0, 0, 0], vec![0, 10, 0, 0], vec![0, 0, 10, 0]],
        support::TOKENIZER_CON_AGREGADO_FUERA_DE_RANGO,
    );
    let p = Pack::open_pack(&r).expect("un added_token fuera de rango no invalida la tabla");
    assert_eq!(p.name(), "agregados-es");
    // And it still embeds just like the usual fixture.
    let v = p.embed("hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
    // The added token has no row and breaks nothing.
    let v = p.embed("<mask> hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
}

// --- A-1: the declared name is a path, and has to be an inert one ---------
//
// `packstore::pack_path` builds `paquetes/{name}.anathem-pack` out of the
// name the file itself declares. `Path::join` neither neutralises `..` nor
// refuses an absolute path: it replaces the base with it. So a name is not a
// label, it is a write primitive, and `open_pack` is the one place that can
// refuse it before any `Pack` exists carrying it.

/// Every shape that would escape `paquetes/`, and the reason it would.
fn hostile_names() -> Vec<(&'static str, &'static str)> {
    vec![
        ("../../../../tmp/escapado", "parent directories"),
        ("..", "the parent itself"),
        (".", "the directory itself"),
        ("/tmp/absoluto", "an absolute path replaces the base"),
        ("sub/dir", "a separator makes two components"),
        ("sub\\dir", "the Windows separator, on any platform"),
        (".oculto", "a leading dot"),
        ("C:windows", "a Windows drive prefix"),
        ("", "the empty name"),
    ]
}

#[test]
fn it_rejects_a_name_that_would_not_be_a_plain_file_name() {
    for (name, por_que) in hostile_names() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("hostil.anathem-pack");
        support::write_pack_mini(&r, name);
        let e = Pack::open_pack(&r)
            .expect_err(&format!("tendría que rechazar {name:?}: {por_que}"))
            .to_string();
        assert!(
            e.contains("name"),
            "el error debe decir que el problema es el nombre ({por_que}): {e}"
        );
        assert!(
            e.contains("hostil.anathem-pack"),
            "el error debe nombrar el archivo ({por_que}): {e}"
        );
    }
}

#[test]
fn it_accepts_the_names_real_packs_use() {
    for name in ["en-es", "en_es", "es", "paquete.v2", "EN-ES-2026"] {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("bueno.anathem-pack");
        support::write_pack_mini(&r, name);
        let p = Pack::open_pack(&r)
            .unwrap_or_else(|e| panic!("{name:?} es un nombre legítimo y fue rechazado: {e}"));
        assert_eq!(p.name(), name);
    }
}
