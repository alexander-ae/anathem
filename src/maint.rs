use anyhow::Result;
use rusqlite::{params, Connection};

/// Marca una memoria como `forgotten`. No borra la fila: `search::run` filtra
/// por `status = 'active'`, así que basta con cambiar el status para sacarla
/// de la búsqueda sin perder el contenido ni romper referencias (`supersedes_id`,
/// `memory_refs`, etc.) que otras filas puedan tener hacia ella.
///
/// `UPDATE ... WHERE id = ?` no falla en SQLite si no matchea ninguna fila —
/// devuelve 0 filas afectadas y sigue. Comprobamos ese conteo explícitamente
/// y fallamos con `bail!` nombrando el id, igual que `supersede::link` (Task
/// 12) ya hace con su `nueva` inexistente: un comando de mantenimiento cuyo
/// único propósito es mutar estado y reportarlo no puede reportar éxito
/// sobre una fila que no existe.
pub fn forget(conn: &Connection, id: i64) -> Result<()> {
    let n = conn.execute("update memories set status='forgotten', updated=?2 where id=?1",
        params![id, chrono::Local::now().format("%Y-%m-%d").to_string()])?;
    if n == 0 {
        anyhow::bail!("no existe la memoria id:{id}");
    }
    Ok(())
}

/// Fija o quita el flag `pinned` de una memoria. Ver `forget` sobre por qué
/// se comprueba el conteo de filas afectadas en vez de confiar en que
/// `execute` no haya devuelto un error.
pub fn pin(conn: &Connection, id: i64, on: bool) -> Result<()> {
    let n = conn.execute("update memories set pinned=?2 where id=?1",
        params![id, if on { 1 } else { 0 }])?;
    if n == 0 {
        anyhow::bail!("no existe la memoria id:{id}");
    }
    Ok(())
}

/// Devuelve (id, title, status) desde `id` hacia atrás por la cadena de supersession.
///
/// `supersede::link` (Task 12) ya rechaza cualquier `link` cuya `nueva` no esté
/// `active`, lo que hace estructuralmente imposible cerrar un ciclo por esa vía:
/// la arista de cierre de un ciclo de k nodos siempre tendría una `nueva` que ya
/// quedó `superseded` por una vuelta anterior del mismo ciclo. Así que un ciclo
/// debería ser inalcanzable recorriendo este código.
///
/// Igual mantenemos la guarda de corte (`out.len() > 50`) porque los datos
/// pueden llegar por otras vías que no pasan por `link`: una base editada a
/// mano, un import futuro, un bug en una tarea posterior. Un guardia que nunca
/// se dispara cuesta una comparación; su ausencia costaría un loop infinito.
pub fn history(conn: &Connection, id: i64) -> Result<Vec<(i64, String, String)>> {
    let mut out = Vec::new();
    let mut cur = Some(id);
    while let Some(c) = cur {
        let row: rusqlite::Result<(i64, String, String, Option<i64>)> = conn.query_row(
            "select id, title, status, supersedes_id from memories where id=?1",
            [c], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
        match row {
            Ok((i, t, s, prev)) => { out.push((i, t, s)); cur = prev; }
            Err(_) => break,
        }
        if out.len() > 50 { break; } // corta ciclos
    }
    Ok(out)
}

/// Una clave de tema tal como la lista `mem topics`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tema {
    pub project: String,
    pub topic: String,
    /// Cuántas memorias tuvo la clave en total, contando las ya reemplazadas
    /// y las olvidadas: es el largo de su historia.
    pub total: i64,
    /// La memoria activa que hoy responde por la clave. `None` si todas
    /// quedaron olvidadas: la clave existe pero no hay versión vigente.
    pub vigente: Option<i64>,
}

/// Las claves de tema que existen, ordenadas por proyecto y clave. La misma
/// clave en dos proyectos son dos filas, porque son dos temas.
pub fn temas(conn: &Connection) -> Result<Vec<Tema>> {
    let mut stmt = conn.prepare(
        "select project, topic, count(*),
                max(case when status = 'active' then id end)
           from memories
          where topic is not null
          group by project, topic
          order by project, topic",
    )?;
    let filas = stmt
        .query_map([], |r| {
            Ok(Tema {
                project: r.get(0)?,
                topic: r.get(1)?,
                total: r.get(2)?,
                vigente: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(filas)
}
