//! Lo que pasa cuando hay más de un proceso.
//!
//! Cada tarea del plan protegió el principio de "nunca mezclar vectores"
//! dentro de su propio proceso, y ninguna tuvo dos procesos en su diff. Estos
//! tests lanzan procesos de verdad: un `mem` por terminal, como en la vida
//! real.

mod apoyo;

use std::path::Path;
use std::process::{Command, Stdio};

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

fn lanzar(db: &Path, args: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(db)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Siembra fragmentos sin vector directamente en la base, para que ningún
/// `mem` los haya sincronizado antes de tiempo. Son los "pendientes" que
/// después se van a pelear varios procesos a la vez.
fn sembrar_fragmentos_sin_vector(db: &Path, cuantos: usize) {
    let conn = anathem::db::open(db).unwrap();
    conn.execute(
        "insert into memories (project, title, body, created, updated, source)
         values ('p', 'relleno', 'relleno', '2026-09-01', '2026-09-01', 'migrated')",
        [],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    // En una sola transacción: sembrar decenas de miles de fragmentos de a
    // un commit por fila tardaría más que el escenario que se quiere medir.
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
fn varios_save_simultaneos_no_fallan_ni_pierden_vectores() {
    // `sincronizar` calculaba los pendientes fuera de la transacción y los
    // insertaba con un `insert` pelado sobre una clave primaria: con varios
    // procesos a la vez, los que perdían la carrera salían con código 1 y
    // `UNIQUE constraint failed` aunque la memoria sí se había guardado, y
    // sus fragmentos se perdían con el rollback. Por MCP el agente recibía
    // `-32603 error interno` por un guardado exitoso.
    //
    // Además embebía DENTRO de la transacción de escritura, así que el
    // lock quedaba tomado todo lo que tardara el corpus entero — y sin
    // `busy_timeout`, el otro proceso fallaba de inmediato en vez de
    // esperar su turno.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["model", "install", r.to_str().unwrap()]).status.success());

    // Suficientes fragmentos como para que la ventana entre "ver qué falta"
    // y "escribirlo" no sea instantánea.
    sembrar_fragmentos_sin_vector(&db, 600);

    let hijos: Vec<_> = (0..6)
        .map(|i| lanzar(&db, &["save", &format!("hola mundo guardado numero {i}")]))
        .collect();
    for (i, h) in hijos.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "el `mem save` {i} salió con error aunque la memoria se guarda igual: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    let conn = rusqlite::Connection::open(&db).unwrap();
    let guardadas: i64 = conn
        .query_row("select count(*) from memories where source = 'explicit'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(guardadas, 6, "se tienen que haber guardado las seis memorias");
    let sin_vector: i64 = conn
        .query_row(
            "select count(*) from chunks c
               left join chunk_vectors v on v.chunk_id = c.id
              where v.chunk_id is null",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sin_vector, 0, "quedaron {sin_vector} fragmentos sin vector");
}

#[test]
fn buscar_no_falla_aunque_no_pueda_reindexar() {
    // La guarda de identidad convertía a `mem search` en un escritor que
    // renombraba el archivo y reindexaba el corpus entero, y propagaba con
    // `?` todos los errores de ese camino. Con `paquetes/` de solo lectura,
    // buscar salía con 1 y no imprimía nada, permanentemente — contra el
    // docstring de la propia función y contra la fila de la especificación
    // que dice que buscar nunca falla por la capa vectorial.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola mundo"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());

    // El archivo instalado pasa a ser, en realidad, otro paquete: la guarda
    // se dispara y el reindexado necesita renombrarlo.
    let paquetes = d.path().join("paquetes");
    apoyo::escribir_pack_permutado(&paquetes.join("mini-es.anathem-pack"), "otro-es");

    // El rename tiene que fallar, y tiene que fallar para cualquiera. Antes
    // esto era `paquetes/` en modo 0555, que a root no lo detiene: en CI
    // dentro de un contenedor —el caso habitual— el test se saltaba entero y
    // el único escenario determinista de "buscar no falla aunque no pueda
    // reindexar" quedaba cubierto solo por una carrera.
    //
    // El destino del rename ocupado por un directorio NO VACÍO falla siempre,
    // sin depender de permisos: `rename(2)` de un archivo sobre un directorio
    // devuelve EISDIR —y ENOTEMPTY si el kernel llegara a mirar el contenido
    // primero— también para root. Ningún uid puede reemplazar un directorio
    // poblado por un archivo; no es una comprobación de permisos.
    let destino = paquetes.join("otro-es.anathem-pack");
    std::fs::create_dir(&destino).unwrap();
    std::fs::write(destino.join("ocupado"), b"x").unwrap();

    let o = mem(&db, &["search", "hola"]);

    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(o.status.success(), "buscar no puede fallar por la capa vectorial: {err}");
    assert!(out.contains("hola"), "tiene que seguir devolviendo resultados de bm25: {out}");
    assert!(err.to_lowercase().contains("bm25"), "y decir que quedó degradada: {err}");
    // Y que el escenario se montó de verdad: lo que falló es el rename, no
    // cualquier otra cosa que también degradaría. Sin esto, el test podría
    // volver a ser un no-op sin que nadie se entere — que es justo como se
    // anulaba antes cuando corría como root.
    assert!(
        err.contains("renombrar"),
        "el escenario tiene que ser el rename imposible, no otro fallo: {err}"
    );

    // Y no puede haber quedado nada a medias: el registro sigue nombrando al
    // archivo que existe.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (nombre, _, _) = anathem::vecstore::pack_registrado(&conn).unwrap().unwrap();
    assert!(
        paquetes.join(format!("{nombre}.anathem-pack")).exists(),
        "el registro quedó apuntando a un archivo que no existe: {nombre}"
    );
}

#[test]
fn dos_busquedas_en_paralelo_con_la_guarda_disparada_sobreviven_las_dos() {
    // Las dos ven la incompatibilidad y las dos quieren renombrar y
    // reindexar. La que pierde la carrera se moría en el `rename`. Ahora
    // degrada a bm25, que es un resultado peor pero honesto, y sale con 0.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola mundo"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());
    apoyo::escribir_pack_permutado(
        &d.path().join("paquetes").join("mini-es.anathem-pack"),
        "otro-es",
    );

    let hijos: Vec<_> = (0..2).map(|_| lanzar(&db, &["search", "hola"])).collect();
    for (i, h) in hijos.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        assert!(
            o.status.success(),
            "la búsqueda {i} murió en la carrera por reindexar: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(out.contains("hola"), "la búsqueda {i} no devolvió nada: {out}");
    }
}

/// Cuántos fragmentos sin vector siembra el escenario de la mezcla.
///
/// Tiene que alcanzar para que embeberlos dure bastante más que arrancar un
/// proceso y copiar un paquete de fixture: con veinte mil, el `mem save`
/// tarda alrededor de un segundo y el `mem model install` entra en el medio
/// con un margen de casi diez a uno. Es el único parámetro de este test que
/// habría que subir si alguna vez se pusiera inestable.
const BACKLOG: usize = 20_000;

#[test]
fn un_install_concurrente_no_deja_vectores_de_dos_paquetes() {
    // El agujero: `sincronizar` recibía un `&Pack` y no volvía a mirar el
    // registro nunca. Un proceso que resolvía el paquete A y después embebía
    // un backlog grande seguía escribiendo vectores de A minutos más tarde,
    // aunque otra terminal ya hubiera corrido `mem model install B`: el
    // `adoptar_pack` de B borraba los vectores de A que ya estaban, pero no
    // detenía al que los seguía produciendo, y el `on conflict do nothing`
    // los grababa en silencio al lado de los de B.
    //
    // Los dos paquetes tienen las MISMAS dimensiones a propósito: es el caso
    // real —el formato fija 256 para todo paquete— y el peor, porque ni
    // `buscar` ni `cobertura` distinguen los blobs, y como `sincronizar`
    // saltea los fragmentos que ya tienen vector, ningún `mem reindex`
    // posterior lo corrige. La mezcla es permanente.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es"); // ▁hola -> eje 0, ▁mundo -> eje 1
    apoyo::escribir_pack_permutado(&b, "otro-es"); // ▁hola -> eje 3, ▁mundo -> eje 2
    assert!(mem(&db, &["init"]).status.success());
    // Se instala A con la base vacía, así que no queda nada indexado: el
    // backlog se siembra después, que es la situación de siempre (memorias
    // guardadas sin paquete, o un `install` cortado con Ctrl-C).
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());
    sembrar_fragmentos_sin_vector(&db, BACKLOG);

    // Terminal 1: guarda una memoria, y al terminar arrastra el backlog
    // entero con el paquete A.
    let guardado = lanzar(&db, &["save", "hola mundo"]);
    // Terminal 2: cambia el paquete mientras el otro embebe.
    std::thread::sleep(std::time::Duration::from_millis(100));
    let instalado = lanzar(&db, &["model", "install", b.to_str().unwrap()]);

    let o = guardado.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "el `mem save` tiene que salir con 0: la memoria se guardó igual. {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = instalado.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "el `mem model install` tiene que salir con 0: {}",
        String::from_utf8_lossy(&o.stderr)
    );

    let conn = rusqlite::Connection::open(&db).unwrap();
    let (nombre, _, dims) = anathem::vecstore::pack_registrado(&conn).unwrap().unwrap();
    assert_eq!(nombre, "otro-es", "el último que adoptó fue el install de B");

    // Ningún texto de este test contiene la pieza "s", así que con `otro-es`
    // los ejes 0 y 1 son exactamente cero y con `mini-es` son justo los dos
    // que se encienden. Un solo vector con peso ahí es un vector de A
    // viviendo en un índice que dice ser de B.
    let mut stmt = conn.prepare("select vec from chunk_vectors").unwrap();
    let vectores: Vec<Vec<f32>> = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|b| {
            b.unwrap().chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
        })
        .collect();
    assert!(!vectores.is_empty(), "el install de B tiene que haber dejado su índice");
    let intrusos = vectores.iter().filter(|v| v[0] != 0.0 || v[1] != 0.0).count();
    assert_eq!(
        intrusos,
        0,
        "{intrusos} de {} vectores son del paquete mini-es en un índice registrado como \
         otro-es: la mezcla que no puede pasar nunca",
        vectores.len()
    );

    // Y el desenlace aceptable sí tiene que haber pasado: el índice que
    // queda es el de B, completo, porque su `sincronizar` no lo interrumpió
    // nadie.
    let (con, tot) = anathem::vecstore::cobertura(&conn, dims).unwrap();
    assert_eq!(con, tot, "el paquete que ganó tiene que terminar su propio índice");
}
