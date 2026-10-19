mod apoyo;

use std::process::Command;

fn mem(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn instalar_registra_el_paquete_y_lo_reporta() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("mini-es"), "status debe nombrar el paquete: {txt}");
    assert!(txt.contains("1/1"), "status debe reportar cobertura completa: {txt}");
}

#[test]
fn instalar_rechaza_un_sha256_que_no_coincide() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    std::fs::write(
        d.path().join("mini.anathem-pack.sha256"),
        "0000000000000000000000000000000000000000000000000000000000000000  mini.anathem-pack\n",
    )
    .unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "un sha que no coincide tiene que fallar");
    assert!(String::from_utf8_lossy(&o.stderr).contains("sha256"));
}

#[test]
fn instalar_rechaza_un_archivo_que_no_es_paquete() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("cualquiera.anathem-pack");
    std::fs::write(&r, b"no soy un paquete").unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success());
}

#[test]
fn status_sin_paquete_lo_dice_y_no_falla() {
    let d = tempfile::tempdir().unwrap();
    let o = mem(d.path(), &["model", "status"]);
    assert!(o.status.success());
    let txt = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(txt.contains("sin paquete"), "salió: {txt}");
}

#[test]
fn quitar_el_paquete_vuelve_a_bm25_sin_romper_la_busqueda() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);

    let o = mem(d.path(), &["model", "remove"]);
    assert!(o.status.success());

    let s = mem(d.path(), &["model", "status"]);
    assert!(String::from_utf8_lossy(&s.stdout).contains("sin paquete"));

    let b = mem(d.path(), &["search", "hola"]);
    assert!(b.status.success(), "la búsqueda tiene que seguir andando");
    assert!(String::from_utf8_lossy(&b.stdout).contains("hola"));
}

#[test]
fn si_falta_el_archivo_del_paquete_la_busqueda_degrada_a_bm25() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    std::fs::remove_file(d.path().join("paquetes").join("mini-es.anathem-pack")).unwrap();

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "buscar no puede fallar por un paquete ausente");
    assert!(String::from_utf8_lossy(&o.stderr).contains("aviso"));
}

/// Variante del fixture con otro nombre y otras dimensiones, para forzar la
/// incompatibilidad.
fn escribir_pack_ancho(ruta: &std::path::Path, nombre: &str) {
    // Mismo tokenizador, pero 8 dimensiones en vez de 4.
    let tok = apoyo::TOKENIZER_MINI.as_bytes();
    let dims = 8usize;
    let filas: [[u8; 8]; 4] = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [10, 0, 0, 0, 0, 0, 0, 0],
        [0, 10, 0, 0, 0, 0, 0, 0],
        [0, 0, 10, 0, 0, 0, 0, 0],
    ];
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(filas.len() as u32).to_le_bytes());
    b.extend_from_slice(&(nombre.len() as u32).to_le_bytes());
    b.extend_from_slice(nombre.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims { b.extend_from_slice(&1.0f32.to_le_bytes()); }
    for _ in 0..dims { b.extend_from_slice(&0.0f32.to_le_bytes()); }
    for f in filas { b.extend_from_slice(&f); }
    std::fs::write(ruta, b).unwrap();
}

#[test]
fn cambiar_de_paquete_rehace_el_indice_en_vez_de_mezclar() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    escribir_pack_ancho(&b, "ancho-es");
    mem(d.path(), &["save", "hola mundo"]);

    mem(d.path(), &["model", "install", a.to_str().unwrap()]);
    mem(d.path(), &["model", "install", b.to_str().unwrap()]);

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("ancho-es"), "debe quedar el último instalado: {txt}");
    assert!(txt.contains("1/1"), "el índice debe estar completo y rehecho: {txt}");

    // Y los vectores guardados tienen que ser de 8 dimensiones, no de 4.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "los vectores viejos de 4 dimensiones sobrevivieron");
}

#[test]
fn un_paquete_sustituido_a_mano_dispara_el_reindexado() {
    // El registro dice "mini-es" pero el archivo con ese nombre es en realidad
    // otro paquete. Buscar no puede usar los vectores viejos.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    escribir_pack_ancho(&instalado, "mini-es"); // mismo nombre, otras dimensiones

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "debió reindexar con el paquete que está de verdad");
}

#[test]
fn incompatibilidad_por_nombre_deja_el_registro_consistente_con_el_disco() {
    // El registro dice "mini-es", y el archivo en paquetes/mini-es.anathem-pack
    // tiene las MISMAS dimensiones (4) pero es en realidad "otro-es": mismo
    // largo de vector, vocabulario distinto. Mezclar sería numéricamente
    // válido y semánticamente basura — el caso más peligroso, porque nada en
    // el largo del blob lo delata.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    apoyo::escribir_pack_mini(&instalado, "otro-es"); // mismas dims, otro nombre

    let o1 = mem(d.path(), &["search", "hola"]);
    assert!(o1.status.success(), "{}", String::from_utf8_lossy(&o1.stderr));

    // El registro tiene que quedar apuntando a un archivo que existe de
    // verdad: el nombre registrado y el nombre del archivo en disco tienen
    // que coincidir.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let (nombre, _sha, _dims) = anathem::vecstore::pack_registrado(&conn).unwrap().unwrap();
    assert_eq!(nombre, "otro-es", "el registro debe reflejar el nombre real del paquete");
    let ruta_registrada = d.path().join("paquetes").join(format!("{nombre}.anathem-pack"));
    assert!(
        ruta_registrada.exists(),
        "el archivo tiene que vivir donde dice el registro: {}",
        ruta_registrada.display()
    );

    // Una segunda búsqueda tiene que encontrar el paquete sin volver a avisar
    // que falta el archivo ni degradar a bm25.
    let o2 = mem(d.path(), &["search", "hola"]);
    assert!(o2.status.success(), "{}", String::from_utf8_lossy(&o2.stderr));
    let err2 = String::from_utf8_lossy(&o2.stderr).to_string();
    assert!(
        !err2.contains("falta"),
        "la segunda búsqueda no puede decir que falta el archivo: {err2}"
    );
}

#[test]
fn reinstalar_desde_la_ruta_instalada_no_destruye_el_paquete() {
    // `mem model install paquetes/mini-es.anathem-pack` —reinstalar "para
    // forzar el reindexado", o un script que instala siempre desde la ruta
    // canónica— copiaba el archivo sobre sí mismo y lo truncaba a cero
    // bytes. Y como el `abrir` que lo habría detectado corría DESPUÉS del
    // `olvidar_pack`, el resultado era el paquete destruido en disco, cero
    // vectores, y el registro apuntando a basura con el sha viejo, que no
    // delata nada. Irrecuperable sin una copia externa.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    let tam = std::fs::metadata(&instalado).unwrap().len();
    assert!(tam > 0);

    let o = mem(d.path(), &["model", "install", instalado.to_str().unwrap()]);
    assert!(
        o.status.success(),
        "reinstalar desde la ruta ya instalada tiene que funcionar: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        std::fs::metadata(&instalado).unwrap().len(),
        tam,
        "el paquete quedó truncado en disco"
    );

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("1/1"), "los vectores tienen que seguir estando: {txt}");

    let b = mem(d.path(), &["search", "hola"]);
    assert!(b.status.success());
    let err = String::from_utf8_lossy(&b.stderr).to_string();
    assert!(!err.contains("aviso"), "no puede quedar avisando para siempre: {err}");

    // Y la instalación no deja temporales tirados en `paquetes/`.
    let sobrantes: Vec<String> = std::fs::read_dir(d.path().join("paquetes"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack")
        .collect();
    assert!(sobrantes.is_empty(), "quedaron archivos a medio instalar: {sobrantes:?}");
}

#[test]
fn un_paquete_que_no_reconoce_nada_se_rechaza_sin_tocar_al_que_estaba() {
    // Un paquete cuya tabla no cubre el vocabulario que usa esta memoria
    // instala limpio, reporta cobertura completa y produce solo vectores
    // nulos: la búsqueda vectorial deja de aportar nada y nada lo dice.
    // Encima, instalarlo pisaba al paquete que funcionaba antes de haber
    // comprobado siquiera que el nuevo sirviera.
    let d = tempfile::tempdir().unwrap();
    let bueno = d.path().join("bueno.anathem-pack");
    apoyo::escribir_pack_mini(&bueno, "mini-es");
    let nulo = d.path().join("nulo.anathem-pack");
    apoyo::escribir_pack_nulo(&nulo, "nulo-es");

    mem(d.path(), &["save", "hola mundo"]);
    let o = mem(d.path(), &["model", "install", bueno.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let o = mem(d.path(), &["model", "install", nulo.to_str().unwrap()]);
    assert!(
        !o.status.success(),
        "un paquete que solo produce vectores nulos no puede instalarse en silencio"
    );

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(
        txt.contains("mini-es"),
        "el paquete que estaba tiene que sobrevivir a una instalación rechazada: {txt}"
    );
    assert!(txt.contains("1/1"), "y sus vectores también: {txt}");
}

#[test]
fn status_no_cuenta_como_cubiertos_los_vectores_que_la_busqueda_descarta() {
    // `cobertura` hacía `count(*)`, así que reportaba 1/1 mientras `buscar`
    // descartaba esa misma fila por dimensión incompatible. El único
    // diagnóstico que la especificación designa para hacer visible el hueco
    // informaba salud perfecta justo cuando el hueco era total.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);

    // Un vector de 8 dimensiones donde el paquete activo tiene 4: lo que
    // deja cualquiera de las mezclas que esta rama persigue.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    conn.execute("update chunk_vectors set vec = zeroblob(32)", []).unwrap();
    drop(conn);

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(
        txt.contains("0/1"),
        "un vector que la búsqueda descarta por incompatible no es cobertura: {txt}"
    );
}

/// Cuántas filas `pack_instalando` hay en `meta`. Se consulta con SQL crudo
/// a propósito: así este test no depende de que la API para leer la marca
/// exista, y falla por lo que afirma y no por no compilar.
fn marcas_de_instalacion(db: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row("select count(*) from meta where clave = 'pack_instalando'", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn una_instalacion_cortada_despues_del_rename_no_deja_consultando_el_build_viejo() {
    // Entre el `rename` —que ya reemplazó el archivo en `paquetes/`— y el
    // `adoptar_pack` que iba a describirlo hay una ventana. Un SIGKILL ahí
    // deja el build NUEVO en disco con `meta` describiendo al VIEJO.
    //
    // Es el caso que ninguna guarda ve: si el build nuevo se llama igual y
    // mide igual que el viejo —actualizar `en-es`, el motivo más común para
    // reinstalar— el nombre y las dimensiones coinciden, y el sha registrado
    // se descarta a propósito para no hashear 22 MB en cada arranque.
    // Resultado: consultas embebidas con el paquete nuevo contra vectores
    // del viejo, indefinidamente, y `model status` informando cobertura
    // completa.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let viejo = d.path().join("viejo.anathem-pack");
    let nuevo = d.path().join("nuevo.anathem-pack");
    apoyo::escribir_pack_mini(&viejo, "mini-es"); // ▁hola -> eje 0
    apoyo::escribir_pack_permutado(&nuevo, "mini-es"); // mismo nombre y dims, ▁hola -> eje 3

    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", viejo.to_str().unwrap()]).status.success());
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let v = apoyo::leer_vector(&conn, "select vec from chunk_vectors");
        assert!(v[0] > 0.5, "precondición: el índice es del build viejo, salió {v:?}");
        // Matar el proceso justo entre el rename y el `adoptar_pack` no se
        // puede pedir desde un test. Se hace fallar el `adoptar_pack`, que
        // deja exactamente el mismo estado: el archivo ya reemplazado y
        // `meta` todavía describiendo al viejo.
        conn.execute_batch(
            "create trigger cortar_la_instalacion before delete on chunk_vectors
             begin select raise(abort, 'instalación interrumpida'); end;",
        )
        .unwrap();
    }

    let o = mem(d.path(), &["model", "install", nuevo.to_str().unwrap()]);
    assert!(!o.status.success(), "esta instalación tenía que quedar cortada");
    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    assert_eq!(
        std::fs::read(&instalado).unwrap(),
        std::fs::read(&nuevo).unwrap(),
        "precondición: el rename ya pasó y en disco está el build nuevo"
    );

    {
        // El proceso muerto no deja triggers; este era el instrumento.
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("drop trigger cortar_la_instalacion").unwrap();
    }

    let o = mem(d.path(), &["search", "hola"]);
    assert!(
        o.status.success(),
        "buscar no puede fallar por esto: {}",
        String::from_utf8_lossy(&o.stderr)
    );

    let conn = rusqlite::Connection::open(&db).unwrap();
    let v = apoyo::leer_vector(&conn, "select vec from chunk_vectors");
    assert!(
        v[3] > 0.5,
        "se está consultando con el paquete nuevo contra vectores del viejo: {v:?}"
    );
    drop(conn);
    assert_eq!(
        marcas_de_instalacion(&db),
        0,
        "resuelta la incoherencia, la marca no puede quedar reindexando para siempre"
    );
}

#[test]
fn una_instalacion_completa_no_deja_la_marca_puesta() {
    // La otra mitad: si la marca sobreviviera a una instalación exitosa,
    // cada búsqueda posterior reindexaría el corpus entero para siempre.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", r.to_str().unwrap()]).status.success());

    assert_eq!(marcas_de_instalacion(&db), 0, "la instalación terminó: la marca sobra");
    let o = mem(d.path(), &["search", "hola"]);
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.is_empty(), "una instalación sana no puede dejar avisando a cada búsqueda: {err}");
}

#[test]
fn una_instalacion_que_falla_antes_de_tocar_el_archivo_no_deja_la_marca() {
    // Si el rename ni siquiera llegó a pasar, el archivo del paquete
    // registrado está intacto y la marca no describe ningún riesgo: dejarla
    // puesta costaría un reindexado del corpus entero al pedo.
    //
    // El rename se hace fallar con el destino ocupado por un directorio no
    // vacío, que da EISDIR para cualquier uid y no depende de permisos.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);

    let paquetes = d.path().join("paquetes");
    std::fs::create_dir_all(&paquetes).unwrap();
    let destino = paquetes.join("mini-es.anathem-pack");
    std::fs::create_dir(&destino).unwrap();
    std::fs::write(destino.join("ocupado"), b"x").unwrap();

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "el rename imposible tiene que hacer fallar la instalación");
    assert_eq!(marcas_de_instalacion(&db), 0, "el archivo no se tocó: la marca no corresponde");

    // Y tampoco quedan temporales tirados.
    let sobrantes: Vec<String> = std::fs::read_dir(&paquetes)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack")
        .collect();
    assert!(sobrantes.is_empty(), "quedaron archivos a medio instalar: {sobrantes:?}");
}
