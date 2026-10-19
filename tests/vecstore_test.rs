mod apoyo;

use anathem::{db, ingest, model::NewMemory, pack::Pack, search, vecstore};
use rusqlite::Connection;

fn sembrar(conn: &Connection, cuerpo: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: cuerpo.chars().take(30).collect(),
        body: cuerpo.into(),
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
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    (dir, conn, p)
}

#[test]
fn sincronizar_llena_los_huecos_y_es_idempotente() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    sembrar(&conn, "mundo");
    assert_eq!(apoyo::registrar_y_sincronizar(&conn, &p).escritos(), 2);
    assert_eq!(apoyo::registrar_y_sincronizar(&conn, &p).escritos(), 0, "no debe reembeber");
    assert_eq!(vecstore::cobertura(&conn, p.dims()).unwrap(), (2, 2));
}

#[test]
fn cobertura_muestra_el_hueco_de_lo_guardado_sin_paquete() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    apoyo::registrar_y_sincronizar(&conn, &p);
    sembrar(&conn, "mundo"); // guardada después, sin sincronizar
    assert_eq!(vecstore::cobertura(&conn, p.dims()).unwrap(), (1, 2));
}

#[test]
fn buscar_ordena_por_coseno() {
    let (_d, conn, p) = fixture();
    let hola = sembrar(&conn, "hola");
    let mundo = sembrar(&conn, "mundo");
    apoyo::registrar_y_sincronizar(&conn, &p);
    let q = search::Query { text: String::new(), ..Default::default() };
    let v = p.embeber("hola").unwrap();
    let r = vecstore::buscar(&conn, &v, &q, 10).unwrap();
    assert_eq!(r[0].memory_id, hola, "la más parecida debe ir primera");
    assert!(r[0].coseno > r[1].coseno);
    assert!(r.iter().any(|a| a.memory_id == mundo));
}

#[test]
fn buscar_respeta_el_filtro_de_proyecto() {
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
    apoyo::registrar_y_sincronizar(&conn, &p);
    let q = search::Query { project: Some("uno".into()), ..Default::default() };
    let r = vecstore::buscar(&conn, &p.embeber("hola").unwrap(), &q, 10).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].memory_id, a);
}

#[test]
fn buscar_ignora_las_memorias_no_activas() {
    let (_d, conn, p) = fixture();
    let id = sembrar(&conn, "hola");
    apoyo::registrar_y_sincronizar(&conn, &p);
    conn.execute("update memories set status='forgotten' where id=?1", [id]).unwrap();
    let r = vecstore::buscar(&conn, &p.embeber("hola").unwrap(), &search::Query::default(), 10).unwrap();
    assert!(r.is_empty(), "una memoria olvidada no puede salir en la búsqueda vectorial");
}

#[test]
fn el_registro_del_paquete_va_y_vuelve() {
    let (_d, conn, _p) = fixture();
    assert!(vecstore::pack_registrado(&conn).unwrap().is_none());
    vecstore::registrar_pack(&conn, "en-es", "abc123", 256).unwrap();
    assert_eq!(
        vecstore::pack_registrado(&conn).unwrap(),
        Some(("en-es".into(), "abc123".into(), 256))
    );
    // registrar de nuevo pisa, no duplica
    vecstore::registrar_pack(&conn, "en-es-pt", "def456", 256).unwrap();
    assert_eq!(
        vecstore::pack_registrado(&conn).unwrap(),
        Some(("en-es-pt".into(), "def456".into(), 256))
    );
}

#[test]
fn olvidar_el_paquete_borra_registro_y_vectores() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    apoyo::registrar_y_sincronizar(&conn, &p);
    vecstore::registrar_pack(&conn, "mini-es", "abc", 4).unwrap();
    vecstore::olvidar_pack(&conn).unwrap();
    assert!(vecstore::pack_registrado(&conn).unwrap().is_none());
    assert_eq!(vecstore::cobertura(&conn, p.dims()).unwrap(), (0, 1));
}

// Compañera del test de abajo, para Windows: `#[cfg(unix)]` sacaría el test
// de la compilación entera, y ahí no aparecería ni como "ignored" — solo un
// número total más bajo que nadie va a comparar contra el de Linux. Con esta
// versión, `cargo test` en Windows lo cuenta y lo marca ignorado con el
// motivo a la vista, en vez de hacerlo desaparecer en silencio.
#[cfg(windows)]
#[test]
#[ignore = "el cuerpo real usa PermissionsExt y un $EDITOR de shell (shebang), \
            ninguno de los dos existe en Windows; ver la versión unix de \
            este test, un poco más abajo en este archivo"]
fn editar_una_memoria_invalida_sus_vectores() {
    unimplemented!(
        "esta prueba no puede correr en Windows: depende de PermissionsExt \
         y de un $EDITOR de mentira que es un script de shell"
    )
}

// Se salta en Windows: el `$EDITOR` de mentira es un script de shell al que
// este test le pone el bit ejecutable a mano con `PermissionsExt`, que no
// existe fuera de Unix. Lo que prueba —que `mem edit` sincroniza los
// vectores nuevos— no depende de nada específico de plataforma; es el modo
// de fingir el editor el que sí lo es. No hay una forma portable de armar
// un `$EDITOR` de una línea sin invocar un intérprete de shell. La versión
// de Windows de este test, arriba, deja constancia de que se saltea y por
// qué en vez de desaparecer de la compilación en silencio.
#[cfg(unix)]
#[test]
fn editar_una_memoria_invalida_sus_vectores() {
    // rechunk borra los fragmentos y el cascade se lleva los vectores. Esa
    // mitad siempre anduvo; la que faltaba es que alguien reconstruya los
    // nuevos.
    //
    // La versión anterior de este test llamaba a `sincronizar` a mano
    // después de `replace_body`, así que quedaba verde mientras `mem edit`
    // —el único camino de escritura que no sincronizaba— dejaba la memoria
    // editada sin un solo vector. Daba confianza falsa sobre justo el
    // camino que estaba roto. Ahora corre el binario de verdad.
    let (dir, _conn, p) = fixture();
    let db = dir.path().join("t.db");
    let salida = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).args(["save", "hola"]).output().unwrap();
    assert!(salida.status.success());

    // Instalar el paquete deja la memoria con su vector de "hola".
    let salida = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db)
        .args(["model", "install", dir.path().join("mini.anathem-pack").to_str().unwrap()])
        .output().unwrap();
    assert!(salida.status.success(), "{}", String::from_utf8_lossy(&salida.stderr));

    // Un $EDITOR de mentira que reemplaza el cuerpo por otro texto.
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
    let salida = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(&db).args(["edit", &id.to_string()])
        .env("EDITOR", &editor).output().unwrap();
    assert!(salida.status.success(), "{}", String::from_utf8_lossy(&salida.stderr));

    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        vecstore::cobertura(&conn, p.dims()).unwrap(),
        (1, 1),
        "después de `mem edit` la memoria tiene que quedar con su vector nuevo, \
         no sin vectores hasta el próximo `mem reindex`"
    );
    // Y el vector tiene que ser el del texto NUEVO: "mundo" es el eje 1.
    let blob: Vec<u8> = conn
        .query_row("select vec from chunk_vectors", [], |r| r.get(0))
        .unwrap();
    let v: Vec<f32> = blob.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    assert!(v[1] > 0.9, "el vector debe ser el de \"mundo\" (eje 1), salió {v:?}");
}

#[test]
fn buscar_colapsa_varios_fragmentos_de_la_misma_memoria() {
    // `sembrar` usa cuerpos de 4-5 caracteres, que siempre caben en un solo
    // fragmento (CHUNK_SIZE=500) y nunca ejercitan el colapso por máximo en
    // `buscar` (el `mejor.entry(mid).or_insert(...)`). Esta memoria es
    // deliberadamente larga para forzar al menos dos fragmentos: el primero,
    // relleno de "mundo", y el segundo, el único que contiene "hola" — así el
    // resultado depende de que `buscar` elija el fragmento más parecido y no
    // cualquiera de los dos.
    let (_d, conn, p) = fixture();
    let relleno = "mundo ".repeat(100); // 600 caracteres, puro "mundo"
    let objetivo = "hola ".repeat(50); // 250 caracteres, puro "hola"
    let cuerpo = format!("{relleno}{objetivo}");
    let id = sembrar(&conn, &cuerpo);

    let n_fragmentos: i64 = conn
        .query_row("select count(*) from chunks where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert!(
        n_fragmentos >= 2,
        "la memoria debe partirse en al menos dos fragmentos para que este test sirva de algo; \
         si el chunking cambia y ahora produce {n_fragmentos}, hay que ajustar el cuerpo de prueba"
    );

    apoyo::registrar_y_sincronizar(&conn, &p);

    // Por construcción, "hola" solo aparece en un fragmento: los primeros
    // 500 caracteres caen enteros dentro del relleno de "mundo".
    let esperado_id: i64 = conn
        .query_row(
            "select id from chunks where memory_id = ?1 and body like '%hola%'",
            [id],
            |r| r.get(0),
        )
        .unwrap();

    let q = search::Query::default();
    let r = vecstore::buscar(&conn, &p.embeber("hola").unwrap(), &q, 10).unwrap();
    let aciertos: Vec<&vecstore::Acierto> = r.iter().filter(|a| a.memory_id == id).collect();
    assert_eq!(aciertos.len(), 1, "la memoria debe aparecer una sola vez, no una por fragmento");
    assert_eq!(
        aciertos[0].chunk_id, esperado_id,
        "debe quedarse con el fragmento más parecido, no con cualquiera"
    );
    assert!(aciertos[0].coseno > 0.0, "el fragmento elegido debe tener coseno positivo con \"hola\"");
}

#[test]
fn pack_registrado_propaga_errores_reales_en_vez_de_silenciarlos() {
    // `.ok()` sobre `query_row` convertiría cualquier error real de sqlite
    // (tabla ausente, base corrupta) en `None`, indistinguible de "no hay
    // paquete registrado": la búsqueda degradaría a bm25 en silencio y nadie
    // se enteraría de que la base está rota. Forzamos un error real (falta
    // la tabla `meta`) y confirmamos que se propaga en vez de esconderse.
    let (_d, conn, _p) = fixture();
    conn.execute("drop table meta", []).unwrap();
    let err = vecstore::pack_registrado(&conn).unwrap_err();
    assert!(
        err.to_string().contains("no such table"),
        "un error real de sqlite no debe convertirse en None; error visto: {err}"
    );
}

/// Mete `cuantos` fragmentos sin vector en una memoria de relleno, en una
/// sola transacción. Son el "backlog" que tarda en embeberse y durante el
/// cual el paquete puede cambiar debajo.
fn sembrar_pendientes(conn: &Connection, cuantos: usize) {
    let id = sembrar(conn, "relleno");
    conn.execute("delete from chunks where memory_id = ?1", [id]).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    for i in 0..cuantos {
        tx.execute(
            "insert into chunks (memory_id, ord, body) values (?1, ?2, ?3)",
            rusqlite::params![id, i as i64, format!("hola mundo numero {i}")],
        )
        .unwrap();
    }
    tx.commit().unwrap();
}

#[test]
fn sincronizar_no_escribe_un_solo_vector_con_una_identidad_caducada() {
    // El caso mínimo del defecto: quien llama resolvió el paquete A y, para
    // cuando terminó de embeber, la base ya está registrada con B. Sin la
    // comprobación dentro de la transacción de escritura, los vectores de A
    // entraban en un índice que dice ser de B, y el `on conflict do nothing`
    // ni siquiera daba error. Con las dimensiones iguales —el caso real—
    // nada los distingue después.
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    let caducada = ("mini-es".to_string(), "sha-de-a".to_string(), p.dims());
    vecstore::registrar_pack(&conn, "otro-es", "sha-de-b", p.dims()).unwrap();

    let r = vecstore::sincronizar(&conn, &p, &caducada).unwrap();

    assert_eq!(
        r,
        vecstore::Sincronizacion::Abandonada(0),
        "con la identidad caducada el lote se abandona, no se escribe"
    );
    assert_eq!(
        vecstore::cobertura(&conn, p.dims()).unwrap(),
        (0, 1),
        "no puede haber entrado un solo vector del paquete que ya no está registrado"
    );
}

#[test]
fn sincronizar_abandona_a_mitad_de_camino_si_el_paquete_cambia_entre_lotes() {
    // El caso que de verdad pasa: el backlog es grande, el primer lote se
    // escribe con el paquete correcto y en el medio otra terminal corre
    // `mem model install`. Los lotes siguientes seguían escribiéndose con el
    // paquete viejo al lado de los del nuevo, y como `sincronizar` saltea
    // los fragmentos que ya tienen vector, ningún `mem reindex` posterior lo
    // corregía: la mezcla quedaba para siempre.
    //
    // El cambio "de otro proceso" lo hace un trigger de sqlite sobre
    // `chunk_vectors`, que es la única forma de que el momento exacto sea el
    // mismo en cada corrida: dispara dentro de la transacción del primer
    // lote, igual que un `adoptar_pack` ajeno que gana el lock justo ahí.
    let (_d, conn, p) = fixture();
    let total = 300usize; // más de un lote (el lote son 256)
    sembrar_pendientes(&conn, total);
    let identidad = ("mini-es".to_string(), "sha-de-a".to_string(), p.dims());
    vecstore::registrar_pack(&conn, &identidad.0, &identidad.1, identidad.2).unwrap();
    conn.execute_batch(
        "create trigger cambia_el_paquete after insert on chunk_vectors
         begin
           update meta set valor = 'otro-es' where clave = 'pack_nombre';
         end;",
    )
    .unwrap();

    let r = vecstore::sincronizar(&conn, &p, &identidad).unwrap();

    assert!(r.abandonada(), "el cambio de paquete a mitad de camino tiene que abandonar: {r:?}");
    let escritos: i64 =
        conn.query_row("select count(*) from chunk_vectors", [], |r| r.get(0)).unwrap();
    assert_eq!(escritos as usize, r.escritos(), "lo reportado y lo escrito tienen que coincidir");
    assert!(escritos > 0, "el primer lote sí se escribió con la identidad correcta");
    assert!(
        (escritos as usize) < total,
        "se escribieron los {total} fragmentos con un paquete que ya no era el registrado"
    );
}
