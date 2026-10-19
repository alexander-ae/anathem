//! `mem doctor`: un comando que vuelve visible lo que antes había que ir a
//! buscar a mano. Estos tests invocan el binario real, porque lo que importa
//! es lo que ve (y el código de salida que recibe) quien lo corre.

mod apoyo;

use rusqlite::Connection;
use std::path::Path;
use std::process::Command;

struct Salida {
    stdout: String,
    stderr: String,
    codigo: Option<i32>,
}

impl Salida {
    fn ok(&self) -> bool {
        self.codigo == Some(0)
    }
    /// Lo que vería quien lo corre en una terminal.
    fn todo(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

fn mem(db: &Path, args: &[&str]) -> Salida {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap();
    Salida {
        stdout: String::from_utf8_lossy(&o.stdout).to_string(),
        stderr: String::from_utf8_lossy(&o.stderr).to_string(),
        codigo: o.status.code(),
    }
}

/// Una base con tres memorias: A y B comparten clave de tema (A queda
/// superseded, B vigente) y C es suelta, con un tag explícito.
fn base_sana(dir: &Path) -> std::path::PathBuf {
    let db = dir.join("mem.db");
    assert!(mem(&db, &["init"]).ok());
    for args in [
        &["save", "hola mundo uno", "--project", "p", "--topic", "t"][..],
        &["save", "hola mundo dos", "--project", "p", "--topic", "t"][..],
        &["save", "hola mundo tres", "--project", "p", "--tag", "alfa"][..],
    ] {
        let o = mem(&db, args);
        assert!(o.ok(), "no se pudo preparar la base: {}", o.todo());
    }
    db
}

/// Abre la base para sabotearla, con las claves foráneas apagadas: es la
/// única forma de fabricar el daño que el cascade impediría.
fn sabotear(db: &Path) -> Connection {
    let c = Connection::open(db).unwrap();
    c.execute_batch("pragma foreign_keys = off;").unwrap();
    c
}

fn instalar_paquete(dir: &Path, db: &Path) {
    let pack = dir.join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&pack, "mini-es");
    let o = mem(db, &["model", "install", pack.to_str().unwrap()]);
    assert!(o.ok(), "{}", o.todo());
}

#[test]
fn version_imprime_nombre_version_y_commit() {
    let d = tempfile::tempdir().unwrap();
    let o = mem(&d.path().join("mem.db"), &["--version"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("mem"), "salió: {}", o.stdout);
    assert!(o.stdout.contains(env!("CARGO_PKG_VERSION")), "salió: {}", o.stdout);
    assert!(o.stdout.contains("commit"), "tiene que decir el commit: {}", o.stdout);
}

#[test]
fn base_sana_sale_con_cero_y_reporta_los_conteos() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "una base sana no es un problema (salió {:?}): {}", o.codigo, o.todo());
    assert!(
        o.stdout.contains("2 activas, 1 superseded, 0 draft, 0 forgotten"),
        "conteos por estado: {}",
        o.stdout
    );
    assert!(o.stdout.contains("fragmentos: 3"), "fragmentos: {}", o.stdout);
    assert!(o.stdout.contains("tags: 1"), "tags: {}", o.stdout);
    assert!(o.stdout.contains("referencias: 0"), "referencias: {}", o.stdout);
    assert!(o.stdout.contains(env!("CARGO_PKG_VERSION")), "versión: {}", o.stdout);
    assert!(o.stdout.contains(db.to_str().unwrap()), "la ruta de la base: {}", o.stdout);
    assert!(!o.stdout.contains('✗'), "una base sana no marca nada como mal: {}", o.stdout);
}

#[test]
fn sin_paquete_es_informacion_y_no_un_problema() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("sin paquete"), "{}", o.stdout);
    assert!(o.stdout.contains("léxica"), "tiene que decir que es el modo normal: {}", o.stdout);
}

#[test]
fn base_recien_creada_no_rompe_el_diagnostico() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    mem(&db, &["init"]);
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("0 activas, 0 superseded, 0 draft, 0 forgotten"), "{}", o.stdout);
}

#[test]
fn detecta_el_indice_fts5_desincronizado_y_sale_distinto_de_cero() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    // Saca una fila del índice sin tocar `chunks`: lo que pasaría si un
    // camino de escritura se saltara los triggers.
    let c = sabotear(&db);
    c.execute(
        "insert into chunks_fts(chunks_fts, rowid, body)
         select 'delete', id, body from chunks where id = (select min(id) from chunks)",
        [],
    )
    .unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("FTS5"), "{}", o.stdout);
    assert!(o.stdout.contains("mem reindex"), "tiene que decir cómo se arregla: {}", o.stdout);
}

#[test]
fn config_roto_se_reporta_y_el_diagnostico_no_se_cae() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::write(d.path().join("config.json"), "{ esto no es json").unwrap();

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "un config roto es un problema: {}", o.todo());
    assert!(o.stdout.contains("config.json"), "{}", o.stdout);
    assert!(!o.stderr.contains("panicked"), "no puede entrar en pánico: {}", o.stderr);
    // Y siguió: el resto del diagnóstico está.
    assert!(o.stdout.contains("2 activas"), "tiene que seguir con el resto: {}", o.stdout);
}

#[test]
fn config_con_un_valor_invalido_dice_cual_es() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::write(d.path().join("config.json"), r#"{"candidatos": 0}"#).unwrap();
    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("candidatos"), "{}", o.stdout);
}

#[test]
fn config_valido_muestra_los_valores_efectivos() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::write(d.path().join("config.json"), r#"{"candidatos": 7}"#).unwrap();
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("candidatos=7"), "{}", o.stdout);
    // Los que no se tocaron aparecen con su valor por defecto: eso son los
    // "efectivos".
    assert!(o.stdout.contains("extracto=120"), "{}", o.stdout);
}

#[test]
fn tags_json_roto_se_reporta_y_el_diagnostico_no_se_cae() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::write(d.path().join("tags.json"), "[{\"tag\": \"x\", \"pattern\": \"(\"}]").unwrap();
    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("tags.json"), "{}", o.stdout);
    assert!(!o.stderr.contains("panicked"), "{}", o.stderr);
    assert!(o.stdout.contains("2 activas"), "{}", o.stdout);
}

#[test]
fn paquete_registrado_cuyo_archivo_falta_se_detecta() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    std::fs::remove_file(d.path().join("paquetes").join("mini-es.anathem-pack")).unwrap();

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("mini-es"), "{}", o.stdout);
    assert!(o.stdout.contains("falta"), "{}", o.stdout);
}

#[test]
fn paquete_instalado_y_completo_sale_bien() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("mini-es"), "{}", o.stdout);
    assert!(o.stdout.contains("4 dimensiones"), "{}", o.stdout);
    assert!(o.stdout.contains("3/3"), "cobertura completa: {}", o.stdout);
}

#[test]
fn huecos_de_cobertura_se_detectan() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    let c = sabotear(&db);
    c.execute(
        "delete from chunk_vectors where chunk_id = (select min(id) from chunks)",
        [],
    )
    .unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("2/3"), "{}", o.stdout);
    assert!(o.stdout.contains("mem reindex"), "{}", o.stdout);
}

#[test]
fn un_vector_de_largo_equivocado_no_cuenta_como_cobertura() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    let c = sabotear(&db);
    c.execute(
        "update chunk_vectors set vec = zeroblob(8)
          where chunk_id = (select min(id) from chunks)",
        [],
    )
    .unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("2/3"), "{}", o.stdout);
}

#[test]
fn cadena_de_supersession_hacia_una_memoria_inexistente_se_detecta() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let c = sabotear(&db);
    c.execute("update memories set supersedes_id = 9999 where id = 3", []).unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("9999"), "tiene que nombrar el id que no existe: {}", o.stdout);
    assert!(o.stdout.contains("supersession"), "{}", o.stdout);
}

#[test]
fn dos_activas_con_la_misma_clave_de_tema_se_detectan() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    // Una base vieja, de antes del índice único, podría tenerlo violado.
    let c = sabotear(&db);
    c.execute("drop index idx_mem_topic_activa", []).unwrap();
    c.execute("update memories set status = 'active' where id = 1", []).unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("clave de tema"), "{}", o.stdout);
}

#[test]
fn fragmentos_sin_memoria_y_vectores_sin_fragmento_se_detectan() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    let c = sabotear(&db);
    c.execute("insert into chunks (memory_id, ord, body) values (777, 0, 'huerfano')", [])
        .unwrap();
    c.execute("insert into chunk_vectors (chunk_id, vec) values (888, zeroblob(16))", [])
        .unwrap();
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("fragmentos sin memoria: 1"), "{}", o.stdout);
    assert!(o.stdout.contains("vectores sin fragmento: 1"), "{}", o.stdout);
}

#[test]
fn archivos_laterales_sobrantes_son_un_aviso_y_no_un_problema() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    std::fs::write(
        d.path().join("paquetes").join("mini-es.anathem-pack.reemplazando-4242"),
        b"resto",
    )
    .unwrap();
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "un resto no impide que todo ande: {}", o.todo());
    assert!(o.stdout.contains("reemplazando-4242"), "{}", o.stdout);
}

#[test]
fn base_que_no_existe_todavia_no_rompe_ni_se_crea() {
    let d = tempfile::tempdir().unwrap();
    let carpeta = d.path().join("nueva");
    let db = carpeta.join("mem.db");
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(!o.stderr.contains("panicked"), "{}", o.stderr);
    assert!(o.stdout.contains("no existe"), "{}", o.stdout);
    assert!(!carpeta.exists(), "un diagnóstico no crea nada, ni siquiera la carpeta");
}

#[test]
fn base_inexistente_no_afirma_que_todo_esta_en_orden() {
    // Con un `--db` mal escrito, un script de verificación pasaría en verde
    // sin haber comprobado nada. El código de salida puede ser 0, pero el
    // texto no puede decir que revisó lo que no existe.
    let d = tempfile::tempdir().unwrap();
    let o = mem(&d.path().join("no-esta").join("mem.db"), &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(!o.stdout.contains("Todo en orden"), "{}", o.stdout);
    assert!(o.stdout.contains("Sin problemas ("), "tiene que contar el aviso: {}", o.stdout);
    let linea = o.stdout.lines().find(|l| l.contains("no existe")).unwrap();
    assert!(linea.trim_start().starts_with('!'), "es un aviso, no información: {linea}");
}

#[test]
fn el_resumen_distingue_todo_en_orden_de_sin_problemas_con_avisos() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::create_dir(d.path().join("paquetes")).unwrap();
    std::fs::write(d.path().join("paquetes").join("x.anathem-pack.reemplazando-1"), b"r").unwrap();
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    // (Si el binario se compiló sin git hay un aviso más: no se fija el número.)
    assert!(o.stdout.contains("Sin problemas ("), "{}", o.stdout);
    assert!(!o.stdout.contains("Todo en orden"), "{}", o.stdout);
}

#[test]
fn un_archivo_que_no_es_una_base_se_reporta_y_no_se_cae() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    std::fs::write(&db, b"esto no es sqlite, ni de cerca, ni un poquito").unwrap();
    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(!o.stderr.contains("panicked"), "{}", o.stderr);
    // Y las comprobaciones que no dependen de la base siguen corriendo.
    assert!(o.stdout.contains("config.json"), "{}", o.stdout);
}

#[test]
fn una_base_anterior_a_las_claves_de_tema_no_rompe_el_diagnostico() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let c = sabotear(&db);
    c.execute_batch(
        "drop index idx_mem_topic_activa;
         drop index idx_mem_project_topic;
         alter table memories drop column topic;",
    )
    .unwrap();
    drop(c);
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("2 activas"), "{}", o.stdout);
}

/// Cada archivo del árbol con un resumen de su contenido, para comparar antes
/// y después sin volcar bytes en el mensaje de un fallo.
fn instantanea(dir: &Path) -> std::collections::BTreeMap<String, u64> {
    use std::hash::{Hash, Hasher};
    fn recorrer(dir: &Path, base: &Path, v: &mut std::collections::BTreeMap<String, u64>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().to_string();
            if p.is_dir() {
                v.insert(format!("{rel}/"), 0);
                recorrer(&p, base, v);
            } else {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::fs::read(&p).unwrap().hash(&mut h);
                v.insert(rel, h.finish());
            }
        }
    }
    let mut v = std::collections::BTreeMap::new();
    recorrer(dir, dir, &mut v);
    v
}

#[test]
fn el_diagnostico_es_de_solo_lectura_aun_con_la_base_danada() {
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    instalar_paquete(d.path(), &db);
    // Todo roto a la vez: es cuando más tentador sería "arreglar".
    let c = sabotear(&db);
    c.execute("update memories set supersedes_id = 9999 where id = 3", []).unwrap();
    c.execute(
        "insert into chunks_fts(chunks_fts, rowid, body)
         select 'delete', id, body from chunks where id = (select min(id) from chunks)",
        [],
    )
    .unwrap();
    c.execute("delete from chunk_vectors", []).unwrap();
    drop(c);
    std::fs::write(d.path().join("config.json"), "roto").unwrap();
    std::fs::write(d.path().join("paquetes").join("x.anathem-pack.reemplazando-1"), b"r").unwrap();

    let antes = instantanea(d.path());
    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    let despues = instantanea(d.path());
    let cambios: Vec<_> = antes
        .keys()
        .chain(despues.keys())
        .filter(|k| antes.get(*k) != despues.get(*k))
        .collect();
    assert!(cambios.is_empty(), "doctor modificó el directorio de la base: {cambios:?}");
}

#[test]
fn ve_lo_escrito_por_una_conexion_viva_que_aun_no_consolido_el_wal() {
    // Con el servidor MCP abierto, lo último que se guardó puede estar solo
    // en el `-wal`. Un diagnóstico que lo ignorara mostraría una foto vieja.
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let viva = Connection::open(&db).unwrap();
    viva.execute_batch("pragma journal_mode = wal; pragma wal_autocheckpoint = 0;").unwrap();
    viva.execute(
        "insert into memories (project, title, body, created, updated, source)
         values ('p', 'nueva', 'nueva', '2026-01-01', '2026-01-01', 'explicit')",
        [],
    )
    .unwrap();
    let o = mem(&db, &["doctor"]);
    drop(viva);
    assert!(o.stdout.contains("3 activas"), "{}", o.todo());
}

#[test]
fn una_ruta_con_espacios_y_simbolos_funciona() {
    let d = tempfile::tempdir().unwrap();
    let carpeta = d.path().join("mi base #1 (copia)%");
    std::fs::create_dir(&carpeta).unwrap();
    let db = base_sana(&carpeta);
    let o = mem(&db, &["doctor"]);
    assert!(o.ok(), "{}", o.todo());
    assert!(o.stdout.contains("2 activas"), "{}", o.stdout);
}

#[test]
fn una_base_anterior_a_la_capa_vectorial_es_informacion_y_no_tres_problemas() {
    // Mismo caso que una base anterior a las claves de tema: no está rota,
    // se migra sola en la próxima escritura.
    for tablas in [
        &["chunk_vectors", "meta"][..],
        &["chunk_vectors"][..],
        &["meta"][..],
    ] {
        let d = tempfile::tempdir().unwrap();
        let db = base_sana(d.path());
        let c = sabotear(&db);
        for t in tablas {
            c.execute(&format!("drop table {t}"), []).unwrap();
        }
        drop(c);
        let o = mem(&db, &["doctor"]);
        assert!(o.ok(), "sin {tablas:?}: {}", o.todo());
        assert!(!o.stdout.contains('✗'), "sin {tablas:?}: {}", o.stdout);
        assert!(o.stdout.contains("anterior a la capa vectorial"), "sin {tablas:?}: {}", o.stdout);
        assert!(o.stdout.contains("2 activas"), "sin {tablas:?}: {}", o.stdout);
        // Y es verdad que se migra sola: la próxima escritura la deja sana.
        assert!(mem(&db, &["save", "otra", "--project", "p"]).ok());
        let o = mem(&db, &["doctor"]);
        assert!(!o.stdout.contains("anterior a la capa vectorial"), "{}", o.stdout);
    }
}

#[test]
fn un_fallo_de_doctor_sale_con_2_y_los_problemas_con_1() {
    // Sin HOME y sin --db, la ruta por defecto no se puede resolver: doctor no
    // llega a correr. Un script tiene que poder distinguir eso de "encontró
    // problemas" (1).
    let correr = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_mem"))
            .args(args)
            .env_remove("HOME")
            .env_remove("USERPROFILE")
            .output()
            .unwrap()
    };
    let o = correr(&["doctor"]);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("HOME"));
    // Los demás comandos conservan su 1 de siempre.
    assert_eq!(correr(&["init"]).status.code(), Some(1));

    // Y con problemas reales sigue siendo 1.
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    std::fs::write(d.path().join("config.json"), "roto").unwrap();
    assert_eq!(mem(&db, &["doctor"]).codigo, Some(1));
}

#[test]
fn el_indice_fts5_se_compara_por_ids_y_no_solo_por_cantidad() {
    // Mismo conteo de filas en `chunks` y en el índice, pero no las mismas:
    // se saca el id 1 del índice y se mete un 99 que no existe en `chunks`.
    // Una comparación de conteos diría "sincronizado".
    let d = tempfile::tempdir().unwrap();
    let db = base_sana(d.path());
    let c = sabotear(&db);
    c.execute(
        "insert into chunks_fts(chunks_fts, rowid, body)
         select 'delete', id, body from chunks where id = 1",
        [],
    )
    .unwrap();
    c.execute("insert into chunks_fts(rowid, body) values (99, 'fantasma')", []).unwrap();
    let chunks: i64 = c.query_row("select count(*) from chunks", [], |r| r.get(0)).unwrap();
    let indexados: i64 =
        c.query_row("select count(*) from chunks_fts_docsize", [], |r| r.get(0)).unwrap();
    assert_eq!(chunks, indexados, "el escenario exige igual cantidad");
    drop(c);

    let o = mem(&db, &["doctor"]);
    assert_eq!(o.codigo, Some(1), "{}", o.todo());
    assert!(o.stdout.contains("FTS5 desincronizado"), "{}", o.stdout);
    assert!(o.stdout.contains("1 sin indexar, 1 indexados que ya no existen"), "{}", o.stdout);
}
