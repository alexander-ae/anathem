use anyhow::Result;
use rusqlite::{params, Connection};

pub fn record(
    conn: &Connection,
    query: &str,
    project: Option<&str>,
    ids: &[String],
    budget_used: usize,
) -> Result<i64> {
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values (?1, ?2, ?3, ?4, ?5)",
        params![
            chrono::Local::now().to_rfc3339(),
            query,
            project,
            ids.join(","),
            budget_used as i64
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Marca que el agente pidió el cuerpo completo de una memoria después de
/// buscarla: señal de que el fragmento inyectado no alcanzó.
///
/// `returned_ids` es una lista de ids separados por coma (p.ej. "131,101,1,132").
/// El match tiene que ser sobre entradas delimitadas, no una subcadena: comparar
/// contra `',' || returned_ids || ','` con el patrón `%,id,%` evita que el id 1
/// matchee dentro de 131 o 132.
pub fn mark_follow_up(conn: &Connection, memory_id: i64) -> Result<()> {
    conn.execute(
        "update search_log set followed_up = 1
          where id = (
            select max(id) from search_log
             where ',' || returned_ids || ',' like ?1
          )",
        params![format!("%,{memory_id},%")],
    )?;
    Ok(())
}
