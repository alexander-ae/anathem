mod apoyo;

use anathem::pack::Pack;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn it_opens_and_reports_its_metadata() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    assert_eq!(p.name(), "mini-es");
    assert_eq!(p.dims(), 4);
}

#[test]
fn a_single_token_yields_its_axis() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
    assert!(v[1].abs() < 1e-5 && v[2].abs() < 1e-5 && v[3].abs() < 1e-5);
}

#[test]
fn dos_tokens_promedian_sus_filas() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("hola mundo").unwrap();
    let expected = 1.0f32 / 2f32.sqrt();
    assert!((v[0] - expected).abs() < 1e-5, "salió {v:?}");
    assert!((v[1] - expected).abs() < 1e-5, "salió {v:?}");
}

#[test]
fn the_suffix_is_re_segmented_instead_of_falling_back_to_unknown() {
    // "mundos" no es una pieza, pero "▁mundo" y "s" sí. Esta es la propiedad
    // que hace que recortar el vocabulario no pierda calidad.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::open_pack(&r).unwrap();
    let v = p.embed("mundos").unwrap();
    assert!(v[1] > 0.1 && v[2] > 0.1, "esperaba mezcla de ▁mundo y s, salió {v:?}");
}

#[test]
fn empty_text_yields_a_null_vector_without_failing() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
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
    apoyo::escribir_pack_mini(&r, "mini-es");
    apoyo::corromper(&r, 4, 99); // el primer byte de la versión
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("versión"), "el error debe nombrar la versión: {e}");
}

#[test]
fn it_rejects_a_truncated_file() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let b = std::fs::read(&r).unwrap();
    std::fs::write(&r, &b[..b.len() - 5]).unwrap();
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("truncad"), "el error debe nombrar la causa: {e}");
    assert!(e.contains("mini.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn it_rejects_a_corrupt_tokenizer_length_without_panicking() {
    // "largo del tokenizador" es un u64 leído crudo del archivo. Si un
    // paquete corrupto lo declara cerca de u64::MAX, sumarlo al
    // desplazamiento actual no debe desbordar la aritmética de punteros:
    // bajo overflow-checks (el default de `cargo test`) eso paniquearía con
    // "attempt to add with overflow" en vez de devolver un Err. Corrompemos
    // los 8 bytes del campo para forzar ese valor.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let base = 20 + "mini-es".len(); // desplazamiento del campo tok_len (u64 LE)
    for i in 0..8 {
        apoyo::corromper(&r, base + i, 0xFF);
    }
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("tokenizador"), "el error debe nombrar la causa: {e}");
    assert!(e.contains("mini.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn the_sha256_is_stable_and_changes_with_the_content() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let a = anathem::pack::sha256(&r).unwrap();
    assert_eq!(a, anathem::pack::sha256(&r).unwrap());
    assert_eq!(a.len(), 64);
    apoyo::corromper(&r, 40, 7);
    assert_ne!(a, anathem::pack::sha256(&r).unwrap());
}

#[test]
fn it_rejects_a_table_smaller_than_the_vocabulary() {
    // Un paquete con menos filas que piezas tiene su tokenizador abre
    // limpio, instala, reporta cobertura completa y produce SOLO vectores
    // nulos: `embeber` saltea las filas que no existen, y si faltan todas
    // las saltea todas. Ni una señal de que el paquete no sirve para nada.
    // El `continue` de `embeber` es la red correcta contra el pánico; la
    // incoherencia se detecta al abrir, una sola vez y nombrando el archivo.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("corto.anathem-pack");
    apoyo::escribir_pack_filas(&r, "corto-es", 4, &[vec![0, 0, 0, 0], vec![10, 0, 0, 0]]);
    let e = Pack::open_pack(&r).unwrap_err().to_string();
    assert!(e.contains("tokenizador"), "el error debe explicar la incoherencia: {e}");
    assert!(e.contains("corto.anathem-pack"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn it_accepts_an_added_token_whose_id_is_outside_the_table() {
    // La tabla cubre exactamente las cuatro piezas del modelo, que es lo que
    // el generador escribe. El tokenizador trae además un `added_token` con
    // id 7, fuera de ese rango — la forma de cualquier sentencepiece podado.
    //
    // La comprobación medía `get_vocab_size(true)`, la UNIÓN del vocabulario
    // del modelo con los `added_tokens`: daba 5 contra 4 filas y rechazaba un
    // paquete sano. La cantidad correcta es la del vocabulario del modelo;
    // los ids agregados que caen fuera de la tabla los saltea `embeber`.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("agregados.anathem-pack");
    apoyo::escribir_pack_con_tokenizador(
        &r,
        "agregados-es",
        4,
        &[vec![0, 0, 0, 0], vec![10, 0, 0, 0], vec![0, 10, 0, 0], vec![0, 0, 10, 0]],
        apoyo::TOKENIZER_CON_AGREGADO_FUERA_DE_RANGO,
    );
    let p = Pack::open_pack(&r).expect("un added_token fuera de rango no invalida la tabla");
    assert_eq!(p.name(), "agregados-es");
    // Y sigue embebiendo igual que el fixture de siempre.
    let v = p.embed("hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
    // El token agregado no tiene fila y no rompe nada.
    let v = p.embed("<mask> hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
}
