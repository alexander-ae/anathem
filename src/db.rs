use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
pragma journal_mode = wal;
pragma foreign_keys = on;

create table if not exists memories (
  id            integer primary key,
  project       text not null,
  title         text not null,
  body          text not null,
  created       text not null,
  updated       text not null,
  event_start   text,
  event_end     text,
  source        text not null check (source in ('explicit','suggested','migrated')),
  status        text not null default 'active'
                check (status in ('active','superseded','draft','forgotten')),
  pinned        integer not null default 0,
  last_accessed text,
  access_count  integer not null default 0,
  supersedes_id integer references memories(id),
  ext_id        text unique,
  topic         text   -- clave de tema; ver `ingest::guardar_con_tema`
);

create table if not exists memory_tags (
  memory_id integer not null references memories(id) on delete cascade,
  tag       text not null,
  primary key (memory_id, tag)
);

create table if not exists memory_refs (
  memory_id integer not null references memories(id) on delete cascade,
  kind      text not null check (kind in ('ticket','pr','sha')),
  value     text not null,
  primary key (memory_id, kind, value)
);

create table if not exists chunks (
  id        integer primary key,
  memory_id integer not null references memories(id) on delete cascade,
  ord       integer not null,
  body      text not null
);

create virtual table if not exists chunks_fts using fts5(
  body,
  content='chunks',
  content_rowid='id',
  tokenize="unicode61 remove_diacritics 2"
);

create table if not exists chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);

create table if not exists meta (
  key text primary key,
  value text not null
);

create table if not exists search_log (
  id           integer primary key,
  ts           text not null,
  query        text not null,
  project      text,
  returned_ids text not null,
  budget_used  integer not null,
  followed_up  integer not null default 0
);

create index if not exists idx_mem_project_status on memories(project, status);
create index if not exists idx_chunks_memory      on chunks(memory_id);
create index if not exists idx_tags_tag           on memory_tags(tag);
create index if not exists idx_refs_value         on memory_refs(value);

create trigger if not exists chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
create trigger if not exists chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
end;
create trigger if not exists chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
"#;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    // Varios procesos comparten la base: dos `mem save` desde dos terminales,
    // el servidor MCP y la CLI a la vez, un `mem model install` mientras algo
    // busca. Sin esto, el segundo escritor recibe `database is locked` al
    // instante en vez de esperar su turno.
    //
    // Cinco segundos. Las transacciones de escritura son cortas a propósito
    // —`vecstore::sincronizar` embebe afuera y escribe por lotes de 256— así
    // que cinco segundos cubren decenas de lotes ajenos seguidos, que es
    // mucho más de lo que dura cualquier contención real. Más alto haría que
    // un lock genuinamente trabado pareciera un comando colgado; más bajo
    // volvería a fallar por una carrera de milisegundos.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    agregar_columna_topic(&conn)?;
    agregar_columnas_de_traspaso(&conn)?;
    Ok(conn)
}

fn tiene_columna(conn: &Connection, nombre: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "select count(*) from pragma_table_info('memories') where name = ?1",
        [nombre],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn tiene_columna_topic(conn: &Connection) -> Result<bool> {
    tiene_columna(conn, "topic")
}

/// Agrega una columna que puede faltar en una base vieja, tolerando que otro
/// proceso gane la carrera. Mismo razonamiento que `agregar_columna_topic`:
/// la CLI y el servidor MCP pueden abrir la misma base a la vez, los dos ven
/// la columna ausente, y el segundo `alter` falla con "duplicate column
/// name". Si tras fallar la columna ya está, el resultado es el que se quería.
fn agregar_columna(conn: &Connection, nombre: &str, tipo: &str) -> Result<()> {
    if tiene_columna(conn, nombre)? {
        return Ok(());
    }
    let sql = format!("alter table memories add column {nombre} {tipo}");
    if let Err(e) = conn.execute(&sql, []) {
        if !tiene_columna(conn, nombre)? {
            return Err(e.into());
        }
    }
    Ok(())
}

/// Las columnas y tablas que nacieron con nombres en español, antes de que el
/// proyecto unificara el código en inglés. Se renombran una sola vez; después
/// esto no hace nada.
///
/// Se usa `alter table ... rename column`, que SQLite soporta desde la 3.25 y
/// preserva los datos: no se copia nada ni se reconstruye la tabla, así que no
/// hay ventana donde se puedan perder filas. Si la columna vieja ya no está
/// —el caso de todas las bases nuevas— no se toca nada.
fn renombrar_columnas_al_ingles(conn: &Connection) -> Result<()> {
    for (tabla, viejo, nuevo) in [
        ("memories", "agente", "agent"),
        ("memories", "creado_ts", "created_ts"),
        ("meta", "clave", "key"),
        ("meta", "valor", "value"),
    ] {
        let existe: i64 = conn.query_row(
            "select count(*) from pragma_table_info(?1) where name = ?2",
            [tabla, viejo],
            |r| r.get(0),
        )?;
        if existe == 0 {
            continue;
        }
        // El índice viejo referencia la columna por su nombre anterior; se
        // saca antes de renombrar y se vuelve a crear más abajo con el nuevo.
        conn.execute("drop index if exists idx_mem_project_creado", [])?;
        let sql = format!("alter table \"{tabla}\" rename column \"{viejo}\" to \"{nuevo}\"");
        if let Err(e) = conn.execute(&sql, []) {
            // Otro proceso pudo ganar la carrera, igual que con las columnas
            // nuevas: si ya quedó renombrada, el resultado es el que se quería.
            let ya: i64 = conn.query_row(
                "select count(*) from pragma_table_info(?1) where name = ?2",
                [tabla, nuevo],
                |r| r.get(0),
            )?;
            if ya == 0 {
                return Err(e.into());
            }
        }
    }
    Ok(())
}

/// Las dos columnas que hacen posible contar qué pasó últimamente y quién lo
/// escribió. Son opcionales en el sentido fuerte: una base donde nadie las
/// use se comporta igual que antes, y las memorias viejas las tienen nulas.
///
/// `creado_ts` existe porque `created` guarda solo la fecha. Para saber qué
/// hizo el otro agente hace un rato, un día de resolución no alcanza.
///
/// `agente` lo llena el servidor MCP con el nombre que el cliente manda en el
/// `initialize`; lo guardado desde la CLI queda sin agente, que es la verdad.
fn agregar_columnas_de_traspaso(conn: &Connection) -> Result<()> {
    renombrar_columnas_al_ingles(conn)?;
    agregar_columna(conn, "agent", "text")?;
    agregar_columna(conn, "created_ts", "text")?;
    // Sirve a la consulta de `mem recent`, que filtra por proyecto y ordena
    // por fecha descendente.
    conn.execute(
        "create index if not exists idx_mem_project_created on memories(project, created_ts)",
        [],
    )?;
    Ok(())
}

/// Una base creada antes de las claves de tema no tiene `memories.topic`, y
/// `create table if not exists` no la toca: hay que agregarla con `alter
/// table`. SQLite no tiene `add column if not exists`, así que se pregunta
/// primero. El índice se crea acá y no en `SCHEMA` porque en una base vieja
/// la columna todavía no existe cuando corre ese lote.
///
/// Dos procesos pueden abrir la misma base vieja a la vez (la CLI y el
/// servidor MCP): los dos ven la columna ausente y el segundo `alter` falla
/// con "duplicate column name". Si tras fallar la columna ya está, el otro
/// ganó la carrera y el resultado es el que se quería.
fn agregar_columna_topic(conn: &Connection) -> Result<()> {
    if !tiene_columna_topic(conn)? {
        if let Err(e) = conn.execute("alter table memories add column topic text", []) {
            if !tiene_columna_topic(conn)? {
                return Err(e.into());
            }
        }
    }
    conn.execute(
        "create index if not exists idx_mem_project_topic on memories(project, topic)",
        [],
    )?;
    // Una clave de tema tiene a lo sumo una memoria activa por proyecto: la
    // vigente. El guardado con clave ya lo mantiene (busca la vigente, la
    // supersede y recién después fija la clave en la nueva, ver
    // `ingest::guardar_con_tema`), pero un invariante que depende de que cada
    // camino se acuerde de cumplirlo es el que alguien rompe sin querer. Con
    // el índice, un camino que dejara dos activas falla en la base en vez de
    // dejar a `search` devolviendo las dos.
    //
    // Los únicos caminos que tocan `status` son `forget` (pasa a `forgotten`)
    // y `supersede::link` (a `superseded`); ninguno vuelve a `active`, así que
    // no hay forma de revivir una memoria y chocar con la vigente. Las filas
    // con `topic` nulo no cuentan: en un índice único los nulos son distintos.
    conn.execute(
        "create unique index if not exists idx_mem_topic_activa
           on memories(project, topic)
           where status = 'active' and topic is not null",
        [],
    )?;
    Ok(())
}
