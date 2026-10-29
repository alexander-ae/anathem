//! Qué pasó últimamente en un proyecto, y quién lo escribió.
//!
//! La búsqueda normal ordena por relevancia: contesta "qué sé sobre esto".
//! Para pasarle el trabajo de un agente a otro hace falta lo contrario, "qué
//! pasó acá recién", que es un orden por fecha y una ventana de tiempo.
//!
//! Es una vista aparte y aditiva: no toca la búsqueda, el guardado ni el
//! esquema de nadie que no la use. Las memorias viejas aparecen igual, con el
//! agente en blanco y la fecha del día, que es lo único que de ellas se sabe.

use anyhow::{bail, Result};
use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reciente {
    pub id: i64,
    pub project: String,
    pub title: String,
    /// `creado_ts` si existe; si no, la fecha de `created`.
    pub cuando: String,
    /// Quién lo guardó, si se supo. Nulo en todo lo guardado por la CLI y en
    /// lo anterior a esta funcionalidad.
    pub agente: Option<String>,
    pub topic: Option<String>,
    pub extracto: String,
}

/// Convierte `6h`, `90m`, `2d` o `45s` en segundos.
///
/// Sin sufijo se asumen horas, que es la unidad del caso que motivó esto:
/// cambiar de agente dentro de la misma jornada.
pub fn duracion_en_segundos(texto: &str) -> Result<i64> {
    let t = texto.trim();
    if t.is_empty() {
        bail!("ventana vacía; usá algo como 6h, 90m o 2d");
    }
    let (numero, factor) = match t.chars().last().unwrap() {
        's' => (&t[..t.len() - 1], 1),
        'm' => (&t[..t.len() - 1], 60),
        'h' => (&t[..t.len() - 1], 3600),
        'd' => (&t[..t.len() - 1], 86_400),
        _ => (t, 3600),
    };
    let n: i64 = numero
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("no entiendo la ventana «{texto}»; probá 6h, 90m o 2d"))?;
    if n <= 0 {
        bail!("la ventana tiene que ser positiva, no «{texto}»");
    }
    Ok(n * factor)
}

/// El momento a partir del cual mirar, en el mismo formato que `creado_ts`.
fn desde_hace(segundos: i64) -> String {
    (chrono::Local::now() - chrono::Duration::seconds(segundos)).to_rfc3339()
}

/// Las memorias activas más recientes, de más nueva a más vieja.
///
/// `project` nulo mira todos los proyectos. `ventana_segundos` nulo no acota
/// por tiempo y deja que mande el límite.
pub fn listar(
    conn: &Connection,
    project: Option<&str>,
    ventana_segundos: Option<i64>,
    limite: usize,
    extracto: usize,
) -> Result<Vec<Reciente>> {
    // `creado_ts` es nulo en lo guardado antes de esta funcionalidad, y ahí
    // solo se tiene la fecha. Ordenar y filtrar por el `coalesce` mezcla los
    // dos formatos: una fecha suelta ("2026-10-28") queda antes que cualquier
    // hora de ese mismo día, que es el orden que uno espera al no saber más.
    let mut sql = String::from(
        "select id, project, title, coalesce(creado_ts, created) as cuando,
                agente, topic, body
           from memories
          where status = 'active'",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(p) = project {
        sql.push_str(" and project = ?");
        args.push(Box::new(p.to_string()));
    }
    if let Some(s) = ventana_segundos {
        sql.push_str(" and cuando >= ?");
        args.push(Box::new(desde_hace(s)));
    }
    sql.push_str(" order by cuando desc, id desc limit ?");
    args.push(Box::new(limite as i64));

    let mut st = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let filas = st.query_map(refs.as_slice(), |r| {
        let body: String = r.get(6)?;
        Ok(Reciente {
            id: r.get(0)?,
            project: r.get(1)?,
            title: r.get(2)?,
            cuando: r.get(3)?,
            agente: r.get(4)?,
            topic: r.get(5)?,
            // Recorte por caracteres y no por bytes: el corpus tiene acentos
            // y cortar a mitad de un UTF-8 rompería el texto.
            extracto: body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(extracto).collect(),
        })
    })?;
    Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Lo mismo, en texto, para la CLI y para lo que un agente lea de un tirón.
pub fn formatear(items: &[Reciente], mostrar_proyecto: bool) -> String {
    if items.is_empty() {
        return "sin memorias en esa ventana.".to_string();
    }
    let mut s = String::new();
    for it in items {
        let quien = it.agente.as_deref().unwrap_or("—");
        let fecha = it.cuando.get(..16).unwrap_or(&it.cuando);
        s.push_str(&format!("{fecha}  [{quien}]"));
        if mostrar_proyecto {
            s.push_str(&format!("  ({})", it.project));
        }
        if let Some(t) = &it.topic {
            s.push_str(&format!("  [tema: {t}]"));
        }
        s.push_str(&format!("\n  id:{}  {}\n", it.id, it.title));
        if !it.extracto.is_empty() && it.extracto != it.title {
            s.push_str(&format!("  {}\n", it.extracto));
        }
        s.push('\n');
    }
    s.trim_end().to_string()
}
