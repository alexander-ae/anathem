//! Persistencia de los vectores y búsqueda por coseno.
//!
//! La búsqueda es por fuerza bruta, sin índice aproximado y sin extensiones de
//! SQLite. Un producto punto contra 100.000 fragmentos son 25 millones de
//! operaciones, unos pocos milisegundos, y evita una extensión compilada que
//! rompería la promesa de un solo binario. Conviene revisarlo por encima del
//! medio millón de fragmentos.

use crate::{pack::Pack, search::Query};
use anyhow::Result;
use rusqlite::{params, types::Value, Connection, Transaction, TransactionBehavior};
use std::collections::HashMap;

/// Cuántos fragmentos embebe y escribe `sincronizar` por vuelta.
///
/// Embeber es lo caro —cien mil fragmentos son minutos— y no toca la base,
/// así que va afuera de la transacción. Lo que queda adentro son lotes
/// cortos: un escritor concurrente espera decenas de milisegundos en vez de
/// minutos. Doscientos cincuenta y seis es el compromiso entre pagar el
/// arranque de la transacción seguido y quedarse con el lock mucho rato.
const LOTE: usize = 256;

/// Abre una transacción de escritura tomando el lock de entrada.
///
/// `begin immediate` en vez del `begin deferred` que da
/// `unchecked_transaction`: con deferred, dos procesos pueden empezar a la
/// vez y descubrir el conflicto recién al escribir, cuando ya es tarde para
/// que `busy_timeout` los haga esperar por turnos.
fn escritura(conn: &Connection) -> rusqlite::Result<Transaction<'_>> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Acierto {
    pub memory_id: i64,
    pub chunk_id: i64,
    pub coseno: f32,
}

fn a_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn de_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn anotar_identidad(
    conn: &Connection,
    nombre: &str,
    sha: &str,
    dims: usize,
) -> rusqlite::Result<()> {
    for (k, v) in [
        ("pack_nombre", nombre.to_string()),
        ("pack_sha256", sha.to_string()),
        ("pack_dims", dims.to_string()),
    ] {
        conn.execute(
            "insert into meta (clave, valor) values (?1, ?2)
             on conflict(clave) do update set valor = ?2",
            params![k, v],
        )?;
    }
    Ok(())
}

pub fn registrar_pack(conn: &Connection, nombre: &str, sha: &str, dims: usize) -> Result<()> {
    anotar_identidad(conn, nombre, sha, dims)?;
    Ok(())
}

/// Deja la base lista para indexar con `nombre`: se lleva los vectores del
/// paquete anterior y anota la identidad del nuevo **en la misma
/// transacción**.
///
/// Separarlo en dos pasos —como estaba— abre una ventana en la que los
/// vectores son de un paquete y el registro nombra a otro. Si el proceso
/// muere ahí, la base queda describiendo un índice que no tiene, y nada en
/// los blobs lo delata cuando las dimensiones coinciden. O pasan las dos
/// cosas o no pasa ninguna.
/// Borra además la marca de instalación en curso, en esa misma transacción:
/// la marca existe para cubrir el tramo que termina justo acá, y dejarla
/// viva un instante más costaría un reindexado innecesario.
pub fn adoptar_pack(conn: &Connection, nombre: &str, sha: &str, dims: usize) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from chunk_vectors", [])?;
    anotar_identidad(&tx, nombre, sha, dims)?;
    tx.execute("delete from meta where clave = ?1", [INSTALANDO])?;
    tx.commit()?;
    Ok(())
}

/// La clave de `meta` que dice "hay una instalación a mitad de camino".
///
/// El archivo del paquete se pone en su lugar con un `rename`, que es
/// atómico, pero el registro que lo describe se escribe después: entre las
/// dos cosas hay una ventana en la que `paquetes/{nombre}.anathem-pack` es
/// el build nuevo y `meta` todavía describe al viejo. Un corte de luz o un
/// SIGKILL ahí deja esa mentira en disco para siempre, y cuando el nombre y
/// las dimensiones no cambian —actualizar el build de `en-es`, el caso más
/// común de reinstalar— la guarda de identidad no la ve, porque el sha
/// registrado se descarta a propósito para no hashear 22 MB en cada
/// arranque. Resultado: consultas embebidas con el paquete nuevo contra
/// vectores del viejo, indefinidamente, con `model status` informando
/// cobertura completa.
///
/// La marca se escribe **antes** del rename y se borra en la misma
/// transacción que adopta el paquete, así que existir es exactamente
/// equivalente a "el archivo puede no ser el que `meta` describe". No
/// acorta la ventana: la hace declarada.
const INSTALANDO: &str = "pack_instalando";

/// Anota que se está por reemplazar el archivo del paquete `nombre`.
pub fn marcar_instalacion(conn: &Connection, nombre: &str) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute(
        "insert into meta (clave, valor) values (?1, ?2)
         on conflict(clave) do update set valor = ?2",
        params![INSTALANDO, nombre],
    )?;
    tx.commit()?;
    Ok(())
}

/// El nombre del paquete cuya instalación quedó a medias, si quedó alguna.
pub fn instalacion_pendiente(conn: &Connection) -> Result<Option<String>> {
    match conn.query_row("select valor from meta where clave = ?1", [INSTALANDO], |r| {
        r.get::<_, String>(0)
    }) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Saca la marca. Para cuando la instalación falló antes de tocar el archivo
/// —el rename no llegó a pasar— o para una marca huérfana que ya se sabe que
/// no describe ningún riesgo.
pub fn limpiar_instalacion(conn: &Connection) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from meta where clave = ?1", [INSTALANDO])?;
    tx.commit()?;
    Ok(())
}

/// Cómo identifica la base al paquete con el que indexó: nombre, sha256 y
/// dimensiones. Dos paquetes distintos no coinciden en las tres cosas, y el
/// sha es la única de las tres que distingue dos builds del mismo nombre y
/// las mismas dimensiones —el caso de actualizar `en-es`—, así que las tres
/// van juntas siempre.
pub type Identidad = (String, String, usize);

pub fn pack_registrado(conn: &Connection) -> Result<Option<Identidad>> {
    // No usar `.ok()`: eso convertiría cualquier error real de sqlite (tabla
    // ausente, base corrupta) en `None`, indistinguible de "no hay paquete
    // registrado". Degradar a bm25 está bien; callarse un error real no.
    let leer = |k: &str| -> Result<Option<String>> {
        match conn.query_row("select valor from meta where clave = ?1", [k], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    };
    match (leer("pack_nombre")?, leer("pack_sha256")?, leer("pack_dims")?) {
        (Some(n), Some(s), Some(d)) => Ok(Some((n, s, d.parse()?))),
        _ => Ok(None),
    }
}

pub fn olvidar_pack(conn: &Connection) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from chunk_vectors", [])?;
    tx.execute(
        "delete from meta
          where clave in ('pack_nombre','pack_sha256','pack_dims','pack_instalando')",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Cómo terminó una corrida de `sincronizar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sincronizacion {
    /// No quedó ningún fragmento sin vector. Se escribieron `n` nuevos.
    Completa(usize),
    /// Otro proceso cambió el paquete registrado mientras este embebía. El
    /// lote en curso se descartó sin escribirlo y quedan fragmentos sin
    /// vector; `n` son los que sí se escribieron mientras la identidad
    /// todavía era la esperada, y que el `adoptar_pack` del otro proceso ya
    /// borró.
    ///
    /// **No es un error.** Quien guardó una memoria la guardó igual, y
    /// quedarse con menos vectores de los debidos es exactamente la
    /// degradación aceptable; la que no lo es sería haber escrito ese lote.
    Abandonada(usize),
}

impl Sincronizacion {
    /// Cuántos vectores se escribieron, haya terminado como haya terminado.
    pub fn escritos(self) -> usize {
        match self {
            Sincronizacion::Completa(n) | Sincronizacion::Abandonada(n) => n,
        }
    }

    pub fn abandonada(self) -> bool {
        matches!(self, Sincronizacion::Abandonada(_))
    }
}

/// Embebe los fragmentos que todavía no tienen vector y devuelve cuántos.
///
/// Es idempotente a propósito: los caminos de escritura (guardar, editar,
/// migrar, reindexar) la llaman al terminar en vez de embeber cada uno por su
/// cuenta, para que no puedan divergir entre sí.
///
/// `esperada` es la identidad con la que la base tenía registrado a `pack`
/// cuando quien llama lo abrió, y es obligatoria: sin ella esta función
/// escribe vectores de un paquete que puede haber dejado de ser el
/// registrado hace rato.
///
/// Es el caso real, no uno rebuscado: embeber un backlog grande —memorias
/// guardadas sin paquete, o un `install` interrumpido— lleva minutos, y en
/// ese rato otra terminal puede correr `mem model install`. `adoptar_pack`
/// borra los vectores y anota el paquete nuevo en una transacción, pero eso
/// no detiene al proceso que ya estaba embebiendo con el viejo: sus lotes
/// siguientes se escribían igual, y el `on conflict do nothing` los grababa
/// en silencio al lado de los del paquete nuevo. Con las dimensiones
/// iguales —el caso real, porque el formato fija 256 para todo paquete— ni
/// `buscar` ni `cobertura` distinguen unos de otros, y como `sincronizar`
/// saltea los fragmentos que ya tienen vector, ningún `mem reindex`
/// posterior los corrige: la mezcla es permanente.
///
/// Por eso la comprobación va **dentro** de la transacción `immediate` de
/// cada lote, que ya tiene el lock de escritura tomado: releer `meta` ahí es
/// un `select` por lote, y garantiza que ningún vector se escribe sin que la
/// identidad con la que se lo calculó sea, en ese mismo instante y bajo ese
/// mismo lock, la registrada. Los lotes ya escritos no hacen falta
/// deshacerlos: quien cambie la identidad pasa por `adoptar_pack`, que borra
/// `chunk_vectors` entero.
pub fn sincronizar(conn: &Connection, pack: &Pack, esperada: &Identidad) -> Result<Sincronizacion> {
    let mut total = 0usize;
    loop {
        // Los pendientes se recalculan en cada vuelta: entre lote y lote,
        // otro proceso pudo haber embebido parte de lo que faltaba.
        let pendientes: Vec<(i64, String)> = conn
            .prepare(
                "select c.id, c.body from chunks c
                   left join chunk_vectors v on v.chunk_id = c.id
                  where v.chunk_id is null
                  order by c.id
                  limit ?1",
            )?
            .query_map([LOTE as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        if pendientes.is_empty() {
            break;
        }
        let mut vectores: Vec<(i64, Vec<u8>)> = Vec::with_capacity(pendientes.len());
        for (id, cuerpo) in &pendientes {
            vectores.push((*id, a_blob(&pack.embeber(cuerpo)?)));
        }
        let tx = escritura(conn)?;
        // Con el lock de escritura ya tomado: ¿el paquete con el que se
        // embebió este lote sigue siendo el registrado? Si no, se abandona
        // sin escribir nada. La transacción muere sin `commit` y no deja
        // rastro.
        if pack_registrado(&tx)?.as_ref() != Some(esperada) {
            return Ok(Sincronizacion::Abandonada(total));
        }
        for (id, blob) in &vectores {
            // Dos cosas pueden haber pasado desde el `select` de arriba, y
            // ninguna es un error de quien guardó una memoria:
            //
            // - Otro proceso embebió el mismo fragmento. `do nothing`: gana
            //   el primero. Sin esto, tres `mem save` simultáneos dejaban a
            //   dos saliendo con código 1 y `UNIQUE constraint failed`
            //   aunque la memoria sí se había guardado, y por MCP el agente
            //   veía `-32603 error interno` por un guardado exitoso.
            // - Un `mem edit` rechunkeó y el fragmento ya no existe. El
            //   `where exists` evita que la clave foránea haga fallar el
            //   insert de un fragmento que nadie va a buscar.
            total += tx.execute(
                "insert into chunk_vectors (chunk_id, vec)
                 select ?1, ?2 where exists (select 1 from chunks where id = ?1)
                 on conflict(chunk_id) do nothing",
                params![id, blob],
            )?;
        }
        tx.commit()?;
        if pendientes.len() < LOTE {
            break;
        }
    }
    Ok(Sincronizacion::Completa(total))
}

/// (fragmentos con vector **usable**, fragmentos totales). Un hueco significa
/// que hay memorias que la capa vectorial no ve todavía.
///
/// "Usable" es literal, y por eso hay que pasarle las dimensiones del
/// paquete activo: `buscar` descarta por incompatible todo vector que no
/// mida `dims` flotantes, así que contarlo sería reportar salud perfecta
/// justo cuando el agujero es total. Con `count(*)` a secas, el único
/// diagnóstico que existe para hacer visible el hueco decía 2/2 mientras la
/// búsqueda vectorial no devolvía absolutamente nada.
pub fn cobertura(conn: &Connection, dims: usize) -> Result<(usize, usize)> {
    let con: i64 = conn.query_row(
        "select count(*) from chunk_vectors where length(vec) = ?1",
        [(dims * 4) as i64],
        |r| r.get(0),
    )?;
    let tot: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0))?;
    Ok((con as usize, tot as usize))
}

/// Las `limite` memorias más parecidas a `consulta`, con los mismos filtros
/// que aplica la búsqueda léxica. Cada memoria aparece una sola vez, con su
/// fragmento más parecido.
pub fn buscar(
    conn: &Connection,
    consulta: &[f32],
    q: &Query,
    limite: usize,
) -> Result<Vec<Acierto>> {
    let mut sql = String::from(
        "select m.id, v.chunk_id, v.vec
           from chunk_vectors v
           join chunks c on c.id = v.chunk_id
           join memories m on m.id = c.memory_id
          where m.status = 'active'",
    );
    let mut ps: Vec<Value> = Vec::new();
    if let Some(p) = &q.project {
        ps.push(Value::Text(p.clone()));
        sql.push_str(&format!(" and m.project = ?{}", ps.len()));
    }
    for t in &q.tags {
        ps.push(Value::Text(t.clone()));
        sql.push_str(&format!(
            " and exists (select 1 from memory_tags mt where mt.memory_id = m.id and mt.tag = ?{})",
            ps.len()
        ));
    }
    let mut stmt = conn.prepare(&sql)?;
    let filas = stmt.query_map(rusqlite::params_from_iter(ps.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))
    })?;

    let mut mejor: HashMap<i64, (i64, f32)> = HashMap::new();
    for fila in filas {
        let (mid, cid, blob) = fila?;
        let v = de_blob(&blob);
        if v.len() != consulta.len() {
            continue; // dimensiones distintas: vector de otro paquete
        }
        let cos: f32 = v.iter().zip(consulta).map(|(a, b)| a * b).sum();
        let e = mejor.entry(mid).or_insert((cid, f32::NEG_INFINITY));
        if cos > e.1 {
            *e = (cid, cos);
        }
    }
    let mut out: Vec<Acierto> = mejor
        .into_iter()
        .map(|(memory_id, (chunk_id, coseno))| Acierto { memory_id, chunk_id, coseno })
        .collect();
    // Mayor coseno primero; a igual coseno, menor id, por la misma razón de
    // determinismo que en `fusion::rrf` y en `search::run_explained`.
    out.sort_by(|a, b| {
        b.coseno
            .partial_cmp(&a.coseno)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.memory_id.cmp(&b.memory_id))
    });
    out.truncate(limite);
    Ok(out)
}
