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
fn install_registers_the_pack_and_reports_it() {
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
fn install_rejects_a_sha256_that_does_not_match() {
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
fn install_rejects_a_file_that_is_not_a_pack() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("cualquiera.anathem-pack");
    std::fs::write(&r, b"no soy un paquete").unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success());
}

#[test]
fn status_with_no_pack_says_so_and_does_not_fail() {
    let d = tempfile::tempdir().unwrap();
    let o = mem(d.path(), &["model", "status"]);
    assert!(o.status.success());
    let txt = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(txt.contains("sin paquete"), "salió: {txt}");
}

#[test]
fn removing_the_pack_falls_back_to_bm25_without_breaking_search() {
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
fn a_missing_pack_file_degrades_search_to_bm25() {
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
fn escribir_pack_ancho(pack_path: &std::path::Path, name: &str) {
    // Mismo tokenizador, pero 8 dimensiones en vez de 4.
    let tok = apoyo::TOKENIZER_MINI.as_bytes();
    let dims = 8usize;
    let rows: [[u8; 8]; 4] = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [10, 0, 0, 0, 0, 0, 0, 0],
        [0, 10, 0, 0, 0, 0, 0, 0],
        [0, 0, 10, 0, 0, 0, 0, 0],
    ];
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    b.extend_from_slice(&(name.len() as u32).to_le_bytes());
    b.extend_from_slice(name.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims { b.extend_from_slice(&1.0f32.to_le_bytes()); }
    for _ in 0..dims { b.extend_from_slice(&0.0f32.to_le_bytes()); }
    for f in rows { b.extend_from_slice(&f); }
    std::fs::write(pack_path, b).unwrap();
}

#[test]
fn switching_packs_rebuilds_the_index_instead_of_mixing() {
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
fn a_pack_swapped_by_hand_triggers_the_reindex() {
    // El registro dice "mini-es" pero el archivo con ese nombre es en realidad
    // otro paquete. Buscar no puede usar los vectores viejos.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    escribir_pack_ancho(&instalado, "mini-es"); // mismo name, otras dimensiones

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "debió reindexar con el paquete que está de verdad");
}

#[test]
fn a_name_mismatch_leaves_the_registry_consistent_with_disk() {
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
    apoyo::escribir_pack_mini(&instalado, "otro-es"); // mismas dims, otro name

    let o1 = mem(d.path(), &["search", "hola"]);
    assert!(o1.status.success(), "{}", String::from_utf8_lossy(&o1.stderr));

    // El registro tiene que quedar apuntando a un archivo que existe de
    // verdad: el nombre registrado y el nombre del archivo en disco tienen
    // que coincidir.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let (name, _sha, _dims) = anathem::vecstore::registered_pack(&conn).unwrap().unwrap();
    assert_eq!(name, "otro-es", "el registro debe reflejar el nombre real del paquete");
    let registered_path = d.path().join("paquetes").join(format!("{name}.anathem-pack"));
    assert!(
        registered_path.exists(),
        "el archivo tiene que vivir donde dice el registro: {}",
        registered_path.display()
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
fn reinstalling_from_the_installed_path_does_not_destroy_the_pack() {
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
    let extras: Vec<String> = std::fs::read_dir(d.path().join("paquetes"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack")
        .collect();
    assert!(extras.is_empty(), "quedaron archivos a medio instalar: {extras:?}");
}

#[test]
fn a_pack_that_recognizes_nothing_is_rejected_without_touching_the_current_one() {
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
fn status_does_not_count_vectors_search_discards_as_covered() {
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
fn install_marks(db: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row("select count(*) from meta where key = 'pack_instalando'", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn an_install_cut_short_after_the_rename_does_not_keep_querying_the_old_build() {
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
    let old = d.path().join("viejo.anathem-pack");
    let new_id = d.path().join("nuevo.anathem-pack");
    apoyo::escribir_pack_mini(&old, "mini-es"); // ▁hola -> eje 0
    apoyo::escribir_pack_permutado(&new_id, "mini-es"); // mismo name y dims, ▁hola -> eje 3

    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", old.to_str().unwrap()]).status.success());
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

    let o = mem(d.path(), &["model", "install", new_id.to_str().unwrap()]);
    assert!(!o.status.success(), "esta instalación tenía que quedar cortada");
    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    assert_eq!(
        std::fs::read(&instalado).unwrap(),
        std::fs::read(&new_id).unwrap(),
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
        install_marks(&db),
        0,
        "resuelta la incoherencia, la marca no puede quedar reindexando para siempre"
    );
}

#[test]
fn a_complete_install_leaves_no_mark_behind() {
    // La otra mitad: si la marca sobreviviera a una instalación exitosa,
    // cada búsqueda posterior reindexaría el corpus entero para siempre.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", r.to_str().unwrap()]).status.success());

    assert_eq!(install_marks(&db), 0, "la instalación terminó: la marca sobra");
    let o = mem(d.path(), &["search", "hola"]);
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.is_empty(), "una instalación sana no puede dejar avisando a cada búsqueda: {err}");
}

#[test]
fn an_install_failing_before_touching_the_file_leaves_no_mark() {
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

    let packs = d.path().join("paquetes");
    std::fs::create_dir_all(&packs).unwrap();
    let target = packs.join("mini-es.anathem-pack");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("ocupado"), b"x").unwrap();

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "el rename imposible tiene que hacer fallar la instalación");
    assert_eq!(install_marks(&db), 0, "el archivo no se tocó: la marca no corresponde");

    // Y tampoco quedan temporales tirados.
    let extras: Vec<String> = std::fs::read_dir(&packs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack")
        .collect();
    assert!(extras.is_empty(), "quedaron archivos a medio instalar: {extras:?}");
}
