//! `mem doctor`: todo lo que antes había que ir a buscar a mano, en un comando.
//!
//! Existe porque tres fallas seguidas fueron silenciosas: un binario
//! instalado que estaba atrás del código, un paquete de vocabulario que
//! quedó desinstalado mientras la búsqueda seguía andando degradada, y un
//! plugin que no levantaba. Cada una costó rato descubrirla, y cada una se
//! habría visto de un vistazo.
//!
//! Código de salida: 0 si no hay problemas (los avisos y la información no
//! cuentan), 1 si los hay, 2 si doctor mismo no pudo correr.
//!
//! Dos reglas ordenan el módulo:
//!
//! 1. **Es de solo lectura.** No repara, no migra, no crea ni la base ni su
//!    carpeta: abre la base con `SQLITE_OPEN_READ_ONLY` y *no* pasa por
//!    `db::open`, que ejecuta el esquema, crea las tablas que falten y agrega
//!    columnas. Un diagnóstico que modifica lo que diagnostica puede esconder
//!    justo el problema que se le pidió encontrar. Si algo está mal, dice qué
//!    comando lo arregla y deja que lo corra una persona.
//! 2. **Nunca se cae por lo que diagnostica.** Cada comprobación atrapa sus
//!    propios errores y los convierte en una línea; un archivo roto, una
//!    tabla ausente o una base que ni es SQLite no impiden que las demás
//!    comprobaciones corran. Un diagnóstico que no arranca cuando hay un
//!    problema no sirve.
//!
//! Un límite que conviene tener presente: la comprobación del índice FTS5
//! verifica **presencia**, no contenido. Compara qué ids de `chunks` están en
//! el índice y viceversa, pero no que el texto indexado sea el de
//! `chunks.body`: si el `body` cambia sin pasar por el trigger, doctor dice
//! "sincronizado" y la búsqueda no encuentra el texto nuevo. Comprobarlo
//! (`integrity-check`) es un `insert`, y doctor no escribe.

use crate::{config, pack::Pack, paquete, tags, vecstore};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// La versión del paquete, la de `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// El commit con el que se compiló, o `desconocido` si no había git (ver
/// `build.rs`).
pub const COMMIT: &str = env!("ANATHEM_COMMIT");

/// Lo que imprime `mem --version`.
pub const VERSION_COMPLETA: &str =
    concat!(env!("CARGO_PKG_VERSION"), " (commit ", env!("ANATHEM_COMMIT"), ")");

/// Código de salida de `mem doctor` cuando encontró problemas.
pub const SALIDA_PROBLEMAS: i32 = 1;

/// Código de salida de `mem doctor` cuando no pudo correr (una ruta de base
/// imposible de resolver, un uso incorrecto de la línea de comandos). Es
/// distinto del anterior para que un script distinga "encontró problemas" de
/// "se rompió". Sin problemas sale con 0.
pub const SALIDA_ROTO: i32 = 2;

/// Cuántos ids concretos se nombran por comprobación antes de resumir con
/// "y N más": alcanza para ir a mirarlos sin inundar la terminal.
const EJEMPLOS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nivel {
    /// Todo en orden.
    Bien,
    /// Información: algo que conviene saber y que no es un problema.
    Info,
    /// Algo raro que no impide que todo ande; no cambia el código de salida.
    Aviso,
    /// Un problema. Hace que `mem doctor` salga con código distinto de cero.
    Mal,
}

impl Nivel {
    fn simbolo(self) -> &'static str {
        match self {
            Nivel::Bien => "✓",
            Nivel::Info => "i",
            Nivel::Aviso => "!",
            Nivel::Mal => "✗",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Linea {
    pub nivel: Nivel,
    pub texto: String,
}

#[derive(Debug, Clone)]
pub struct Seccion {
    pub titulo: &'static str,
    pub lineas: Vec<Linea>,
}

impl Seccion {
    fn nueva(titulo: &'static str) -> Self {
        Seccion { titulo, lineas: Vec::new() }
    }
    fn agregar(&mut self, nivel: Nivel, texto: impl Into<String>) {
        self.lineas.push(Linea { nivel, texto: texto.into() });
    }
    fn bien(&mut self, texto: impl Into<String>) {
        self.agregar(Nivel::Bien, texto);
    }
    fn info(&mut self, texto: impl Into<String>) {
        self.agregar(Nivel::Info, texto);
    }
    fn aviso(&mut self, texto: impl Into<String>) {
        self.agregar(Nivel::Aviso, texto);
    }
    fn mal(&mut self, texto: impl Into<String>) {
        self.agregar(Nivel::Mal, texto);
    }
}

#[derive(Debug, Clone)]
pub struct Informe {
    pub secciones: Vec<Seccion>,
}

impl Informe {
    fn contar(&self, nivel: Nivel) -> usize {
        self.secciones.iter().flat_map(|s| &s.lineas).filter(|l| l.nivel == nivel).count()
    }

    /// Cuántas comprobaciones salieron mal. Es lo que decide el código de
    /// salida: los avisos y la información no cuentan.
    pub fn problemas(&self) -> usize {
        self.contar(Nivel::Mal)
    }

    pub fn avisos(&self) -> usize {
        self.contar(Nivel::Aviso)
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for sec in &self.secciones {
            s.push_str(sec.titulo);
            s.push('\n');
            for l in &sec.lineas {
                s.push_str(&format!("  {} {}\n", l.nivel.simbolo(), l.texto));
            }
            s.push('\n');
        }
        let (p, a) = (self.problemas(), self.avisos());
        s.push_str(&match (p, a) {
            (0, 0) => "Todo en orden.\n".to_string(),
            (0, a) => format!("Sin problemas ({a} {}).\n", plural(a, "aviso", "avisos")),
            (p, 0) => format!("{p} {}.\n", plural(p, "problema", "problemas")),
            (p, a) => format!(
                "{p} {} y {a} {}.\n",
                plural(p, "problema", "problemas"),
                plural(a, "aviso", "avisos")
            ),
        });
        s
    }
}

fn plural(n: usize, uno: &'static str, varios: &'static str) -> &'static str {
    if n == 1 {
        uno
    } else {
        varios
    }
}

/// Corre todas las comprobaciones sobre la base en `db` y el directorio que la
/// contiene. No escribe nada y no falla: lo que no se pueda comprobar queda
/// dicho en el informe.
pub fn diagnosticar(db: &Path) -> Informe {
    // `parent()` de una ruta sin carpeta (`mem.db`) es `""`, que unido a un
    // nombre da una ruta relativa al directorio actual: lo correcto.
    let dir = db.parent().unwrap_or(Path::new("."));
    let mut secciones = vec![binario()];
    let (seccion_base, conn) = base(db);
    secciones.push(seccion_base);
    if let Some(conn) = &conn {
        secciones.push(vectorial(conn, dir));
    }
    secciones.push(configuracion(dir));
    Informe { secciones }
}

fn binario() -> Seccion {
    let mut s = Seccion::nueva("Binario");
    s.bien(format!("mem {VERSION}"));
    if COMMIT == "desconocido" {
        s.aviso(
            "commit desconocido: se compiló sin git a mano, así que no se puede saber \
             si este binario está al día con el código",
        );
    } else {
        s.bien(format!("compilado desde el commit {COMMIT}"));
    }
    s
}

/// `count(*)` y compañía: una consulta que devuelve un entero.
fn entero(conn: &Connection, sql: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [], |r| r.get(0))
}

fn tiene_tabla(conn: &Connection, nombre: &str) -> rusqlite::Result<bool> {
    Ok(conn.query_row(
        "select count(*) from sqlite_master where type in ('table', 'view') and name = ?1",
        [nombre],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// Las tablas de la capa vectorial que una base anterior a ella no tiene. Es
/// el mismo caso que una base anterior a las claves de tema: nada está roto,
/// `db::open` las crea (`create table if not exists`) la próxima vez que un
/// comando abra la base para escribir.
const TABLAS_VECTORIALES: [&str; 2] = ["chunk_vectors", "meta"];

fn tablas_vectoriales_ausentes(conn: &Connection) -> rusqlite::Result<Vec<&'static str>> {
    let mut v = Vec::new();
    for t in TABLAS_VECTORIALES {
        if !tiene_tabla(conn, t)? {
            v.push(t);
        }
    }
    Ok(v)
}

/// Lista ids como `1, 2, 3 y 4 más`, sin pasarse de `EJEMPLOS`.
fn nombrar(ids: &[String]) -> String {
    if ids.len() <= EJEMPLOS {
        ids.join(", ")
    } else {
        format!("{} y {} más", ids[..EJEMPLOS].join(", "), ids.len() - EJEMPLOS)
    }
}

/// Devuelve la primera columna de cada fila como texto.
fn textos(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(sql)?.query_map([], |r| r.get::<_, String>(0))?.collect()
}

/// Cierra una comprobación que puede fallar: si falla, no se cae, lo dice.
fn atrapar(s: &mut Seccion, que: &str, f: impl FnOnce(&mut Seccion) -> anyhow::Result<()>) {
    if let Err(e) = f(s) {
        s.mal(format!("no se pudo comprobar {que}: {e:#}"));
    }
}

/// Abre la base sin poder escribirla, y sin dejar rastro en el directorio.
///
/// `SQLITE_OPEN_READ_ONLY` solo no alcanza: en una base en modo WAL, SQLite
/// crea `mem.db-wal` y `mem.db-shm` aunque la conexión sea de lectura, y como
/// no puede hacer el checkpoint al cerrar, los deja. No es contenido, pero es
/// un diagnóstico que ensucia la carpeta que diagnostica.
///
/// Cuando no hay `-wal` es porque ninguna conexión tiene la base abierta (la
/// última en cerrarse lo borra después de hacer el checkpoint) y todo lo
/// escrito ya está en el archivo principal: ahí se abre `immutable=1`, que ni
/// toma locks ni toca el WAL. Si el `-wal` existe, hay alguien escribiendo o
/// quedó una escritura sin consolidar, y ignorarlo mostraría una foto
/// vieja: ahí se abre normal, que lo lee y no crea nada nuevo.
fn abrir_solo_lectura(db: &Path) -> rusqlite::Result<Connection> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut wal = db.as_os_str().to_os_string();
        wal.push("-wal");
        if !Path::new(&wal).exists() {
            let mut uri = String::from("file:");
            for &b in db.as_os_str().as_bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                        uri.push(b as char)
                    }
                    _ => uri.push_str(&format!("%{b:02X}")),
                }
            }
            uri.push_str("?immutable=1");
            return Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            );
        }
    }
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn base(db: &Path) -> (Seccion, Option<Connection>) {
    let mut s = Seccion::nueva("Base de datos");
    if !db.exists() {
        // Aviso y no información: con un `--db` mal escrito, un `$HOME`
        // distinto o un volumen sin montar, el resumen no puede afirmar que
        // todo está en orden cuando no se revisó ninguna base. El código de
        // salida sigue siendo 0 porque "todavía no instalé nada" es legítimo.
        s.aviso(format!(
            "{} no existe todavía: no hay base que revisar. Se crea con `mem init` o al \
             guardar la primera memoria; si esperabas encontrarla acá, revisá `--db`",
            db.display()
        ));
        return (s, None);
    }
    // Solo lectura, y a propósito no `db::open`: ver la documentación del
    // módulo.
    let conn = match abrir_solo_lectura(db) {
        Ok(c) => c,
        Err(e) => {
            s.mal(format!("no se pudo abrir {}: {e}", db.display()));
            return (s, None);
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // Abrir no lee nada: un archivo que no es SQLite recién se queja en la
    // primera consulta.
    match entero(&conn, "select count(*) from sqlite_master where name = 'memories'") {
        Err(e) => {
            s.mal(format!(
                "{} no se pudo leer como base SQLite: {e}. ¿Es el archivo correcto? \
                 Si está corrupto, restaurá una copia.",
                db.display()
            ));
            return (s, None);
        }
        Ok(0) => {
            s.mal(format!(
                "{} existe pero no tiene el esquema de anathem; corré `mem init`",
                db.display()
            ));
            return (s, None);
        }
        Ok(_) => s.bien(format!("{} abre bien", db.display())),
    }
    comprobar_base(&conn, &mut s);
    (s, Some(conn))
}

fn comprobar_base(conn: &Connection, s: &mut Seccion) {
    match tablas_vectoriales_ausentes(conn) {
        Ok(v) if !v.is_empty() => s.info(format!(
            "la base es anterior a la capa vectorial (faltan las tablas {}); se actualiza \
             sola la próxima vez que un comando la abra para escribir",
            v.join(" y ")
        )),
        Ok(_) => {}
        Err(e) => s.mal(format!("no se pudo comprobar las tablas: {e}")),
    }
    atrapar(s, "los conteos", |s| conteos(conn, s));
    atrapar(s, "el índice FTS5", |s| indice_fts(conn, s));
    atrapar(s, "los huérfanos", |s| huerfanos(conn, s));
    atrapar(s, "las cadenas de supersession", |s| supersession(conn, s));
    atrapar(s, "las claves de tema", |s| claves_de_tema(conn, s));
}

fn conteos(conn: &Connection, s: &mut Seccion) -> anyhow::Result<()> {
    let mut por_estado = [0i64; 4];
    let mut stmt = conn.prepare("select status, count(*) from memories group by status")?;
    let filas = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for f in filas {
        let (estado, n) = f?;
        match estado.as_str() {
            "active" => por_estado[0] = n,
            "superseded" => por_estado[1] = n,
            "draft" => por_estado[2] = n,
            "forgotten" => por_estado[3] = n,
            // El check de la tabla lo impide; si una base vieja trae otro, se
            // sumaría a ninguna columna y el total no cerraría sin explicación.
            otro => s.aviso(format!("{n} memorias con un estado desconocido: {otro}")),
        }
    }
    s.bien(format!(
        "memorias: {} activas, {} superseded, {} draft, {} forgotten",
        por_estado[0], por_estado[1], por_estado[2], por_estado[3]
    ));
    s.bien(format!(
        "fragmentos: {}, tags: {}, referencias: {}",
        entero(conn, "select count(*) from chunks")?,
        entero(conn, "select count(*) from memory_tags")?,
        entero(conn, "select count(*) from memory_refs")?,
    ));
    Ok(())
}

/// Compara los fragmentos con lo que el índice FTS5 tiene indexado.
///
/// **No se puede contar con `select count(*) from chunks_fts`**: `chunks_fts`
/// es de contenido externo (`content='chunks'`) y un recorrido completo lee
/// las filas de `chunks`, no del índice, así que daría siempre el mismo
/// número que `chunks` y nunca detectaría nada. Lo que sí refleja lo que se
/// indexó de verdad es la tabla sombra `chunks_fts_docsize`, que tiene una
/// fila por documento indexado.
fn indice_fts(conn: &Connection, s: &mut Seccion) -> anyhow::Result<()> {
    let fragmentos = entero(conn, "select count(*) from chunks")?;
    let indexados = entero(conn, "select count(*) from chunks_fts_docsize")?;
    let sin_indexar = entero(
        conn,
        "select count(*) from chunks c
          where not exists (select 1 from chunks_fts_docsize d where d.id = c.id)",
    )?;
    let sobrantes = entero(
        conn,
        "select count(*) from chunks_fts_docsize d
          where not exists (select 1 from chunks c where c.id = d.id)",
    )?;
    if sin_indexar == 0 && sobrantes == 0 {
        s.bien(format!("índice FTS5 sincronizado ({indexados} de {fragmentos} fragmentos)"));
    } else {
        s.mal(format!(
            "índice FTS5 desincronizado: {fragmentos} fragmentos y {indexados} indexados \
             ({sin_indexar} sin indexar, {sobrantes} indexados que ya no existen). \
             Corré `mem reindex`"
        ));
    }
    Ok(())
}

fn huerfanos(conn: &Connection, s: &mut Seccion) -> anyhow::Result<()> {
    let sin_memoria = entero(
        conn,
        "select count(*) from chunks c
          where not exists (select 1 from memories m where m.id = c.memory_id)",
    )?;
    // Sin la tabla (base anterior a la capa vectorial) no hay vectores que
    // pudieran quedar huérfanos.
    let sin_fragmento = if tiene_tabla(conn, "chunk_vectors")? {
        entero(
            conn,
            "select count(*) from chunk_vectors v
              where not exists (select 1 from chunks c where c.id = v.chunk_id)",
        )?
    } else {
        0
    };
    if sin_memoria == 0 && sin_fragmento == 0 {
        s.bien("sin huérfanos");
    } else {
        // El cascade lo impide, así que si pasó hay un bug: conviene saberlo.
        s.mal(format!(
            "huérfanos (no debería haber ninguno, el cascade lo impide: hay un bug que \
             reportar) — fragmentos sin memoria: {sin_memoria}, vectores sin fragmento: \
             {sin_fragmento}"
        ));
    }
    Ok(())
}

fn supersession(conn: &Connection, s: &mut Seccion) -> anyhow::Result<()> {
    let rotas = textos(
        conn,
        "select 'memoria ' || m.id || ' reemplaza a la ' || m.supersedes_id
           from memories m
          where m.supersedes_id is not null
            and not exists (select 1 from memories p where p.id = m.supersedes_id)
          order by m.id",
    )?;
    if rotas.is_empty() {
        s.bien("cadenas de supersession íntegras");
    } else {
        s.mal(format!(
            "cadena de supersession rota: {}, que no existe. No hay un comando que lo \
             repare; `mem history <id>` muestra hasta dónde llega la cadena",
            nombrar(&rotas)
        ));
    }
    Ok(())
}

fn claves_de_tema(conn: &Connection, s: &mut Seccion) -> anyhow::Result<()> {
    // Una base de antes de las claves de tema no tiene la columna, y no hay
    // nada que decir de un invariante que todavía no existe. No se agrega
    // acá: arreglarlo es cosa de `db::open`, que lo hace al primer comando
    // que escribe.
    let tiene_columna = entero(
        conn,
        "select count(*) from pragma_table_info('memories') where name = 'topic'",
    )? > 0;
    if !tiene_columna {
        s.info(
            "la base es anterior a las claves de tema; se actualiza sola la próxima vez \
             que un comando la abra para escribir",
        );
        return Ok(());
    }
    let repetidas = textos(
        conn,
        "select '(' || project || ', ' || topic || '): memorias ' || group_concat(id, ', ')
           from memories
          where status = 'active' and topic is not null
          group by project, topic
         having count(*) > 1
          order by project, topic",
    )?;
    if repetidas.is_empty() {
        s.bien("una sola memoria activa por (proyecto, clave de tema)");
    } else {
        s.mal(format!(
            "más de una memoria activa para la misma clave de tema, que el índice único \
             debería impedir: {}. Dejá una con `mem supersede <nueva> <vieja>` o \
             `mem forget <id>`",
            nombrar(&repetidas)
        ));
    }
    Ok(())
}

fn vectorial(conn: &Connection, dir: &Path) -> Seccion {
    let mut s = Seccion::nueva("Capa vectorial");
    // La ausencia de las tablas ya se dijo en la sección de la base; acá solo
    // hace falta no tropezar con ella.
    if tablas_vectoriales_ausentes(conn).map_or(false, |v| !v.is_empty()) {
        s.info("sin paquete de vocabulario: la base todavía no tiene capa vectorial");
        laterales(&mut s, dir);
        return s;
    }
    match vecstore::pack_registrado(conn) {
        Err(e) => s.mal(format!("no se pudo leer el paquete registrado: {e:#}")),
        Ok(None) => s.info(
            "sin paquete de vocabulario: la búsqueda es léxica (bm25), que es el modo \
             normal. Se instala uno con `mem model install <archivo>`",
        ),
        Ok(Some((nombre, _sha, dims))) => {
            s.bien(format!("paquete {nombre}, {dims} dimensiones"));
            archivo_del_paquete(&mut s, dir, &nombre, dims);
            atrapar(&mut s, "la cobertura", |s| cobertura(conn, s, dims));
        }
    }
    // La marca de instalación a medias es de la base, no del archivo: se
    // mira siempre.
    match vecstore::instalacion_pendiente(conn) {
        Ok(Some(n)) => s.aviso(format!(
            "hay una instalación de {n} a medias: el archivo en disco puede no ser el que \
             indexó la base. La próxima búsqueda reindexa sola"
        )),
        Ok(None) => {}
        Err(e) => s.mal(format!("no se pudo leer la marca de instalación: {e:#}")),
    }
    laterales(&mut s, dir);
    s
}

fn archivo_del_paquete(s: &mut Seccion, dir: &Path, nombre: &str, dims: usize) {
    let ruta = paquete::ruta(dir, nombre);
    if !ruta.is_file() {
        s.mal(format!(
            "falta el archivo {}: la búsqueda está degradada a bm25. Reinstalalo con \
             `mem model install <archivo>`",
            ruta.display()
        ));
        return;
    }
    match Pack::abrir(&ruta) {
        Err(e) => s.mal(format!(
            "el archivo {} no se pudo abrir ({e:#}): la búsqueda está degradada a bm25. \
             Reinstalalo con `mem model install <archivo>`",
            ruta.display()
        )),
        Ok(p) if p.nombre() != nombre || p.dims() != dims => s.aviso(format!(
            "el archivo {} es el paquete {} ({} dimensiones) y la base indexó con {nombre} \
             ({dims} dimensiones): la próxima búsqueda reindexa",
            ruta.display(),
            p.nombre(),
            p.dims()
        )),
        Ok(_) => s.bien(format!("archivo presente y legible: {}", ruta.display())),
    }
}

fn cobertura(conn: &Connection, s: &mut Seccion, dims: usize) -> anyhow::Result<()> {
    let (con, tot) = vecstore::cobertura(conn, dims)?;
    let vectores = entero(conn, "select count(*) from chunk_vectors")? as usize;
    let inservibles = vectores.saturating_sub(con);
    let nota = if inservibles > 0 {
        format!(" ({inservibles} vectores no miden {dims} dimensiones y no se usan)")
    } else {
        String::new()
    };
    if con == tot {
        s.bien(format!("cobertura: {con}/{tot} fragmentos con vector usable{nota}"));
    } else {
        s.mal(format!(
            "cobertura: {con}/{tot} fragmentos con vector usable; faltan {}{nota}. \
             Corré `mem reindex`",
            tot - con
        ));
    }
    Ok(())
}

/// Restos de una instalación interrumpida en el directorio de paquetes: el
/// archivo que `paquete::reemplazar` aparta mientras pone el nuevo
/// (`*.reemplazando-*`) y el temporal de `paquete::instalar` (`.instalando-*`).
fn laterales(s: &mut Seccion, dir: &Path) {
    let carpeta = paquete::directorio(dir);
    let Ok(entradas) = std::fs::read_dir(&carpeta) else {
        return;
    };
    let mut sobrantes: Vec<String> = entradas
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".reemplazando-") || n.starts_with(".instalando-"))
        .collect();
    sobrantes.sort();
    if sobrantes.is_empty() {
        s.bien("sin archivos laterales sobrantes en el directorio de paquetes");
    } else {
        s.aviso(format!(
            "archivos laterales sobrantes en {}: {}. Son restos de una instalación \
             interrumpida; si no hay ninguna en curso, se pueden borrar",
            carpeta.display(),
            nombrar(&sobrantes)
        ));
    }
}

fn configuracion(dir: &Path) -> Seccion {
    let mut s = Seccion::nueva("Configuración");

    let ruta = dir.join("config.json");
    // `cargar` falla ante un archivo inválido: acá se atrapa y se reporta.
    match config::cargar(dir) {
        Ok(c) => {
            let valores = format!(
                "peso_bm25={:?}, candidatos={}, extracto={}",
                c.peso_bm25, c.candidatos, c.extracto
            );
            if ruta.exists() {
                s.bien(format!("{} válido: {valores}", ruta.display()));
            } else {
                s.bien(format!("sin config.json: valores por defecto ({valores})"));
            }
        }
        Err(e) => s.mal(format!(
            "config.json inválido: {e:#}. `mem search`, `mem eval` y `mem serve` no \
             arrancan mientras tanto; corregilo o borralo"
        )),
    }

    let ruta = dir.join("tags.json");
    match tags::load_rules(Some(dir)) {
        Ok(reglas) => {
            if ruta.exists() {
                s.bien(format!("{} válido: {} reglas", ruta.display(), reglas.len()));
            } else {
                s.bien(format!(
                    "sin tags.json: taxonomía por defecto ({} reglas)",
                    reglas.len()
                ));
            }
        }
        Err(e) => s.mal(format!("tags.json inválido: {e}. Corregilo o borralo")),
    }
    s
}
