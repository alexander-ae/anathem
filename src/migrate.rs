use crate::{ingest, model::NewMemory, tags};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Deserialize;
use std::{fs::File, io::{BufRead, BufReader}, path::Path};

const CURADAS: &[&str] = &["project-knowledge", "project-config", "manual", "learned-pattern"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrateReport {
    pub migradas: usize,
    pub archivadas: usize,
    pub fallidas: usize,
    /// (ext_id, title) de documentos `failed` cuyo tipo es curado: su contenido
    /// está intacto en el export pero no se migran — hay que reimportarlos a mano.
    pub curated_failed: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    content: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    metadata: Option<Meta>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "type")]
    kind: Option<String>,
    project: Option<String>,
}

fn title_from(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.chars().take(120).collect()
}

pub fn run(conn: &Connection, path: &Path) -> Result<MigrateReport> {
    let file = File::open(path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
    let mut rep = MigrateReport::default();

    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: Row = serde_json::from_str(&line)
            .with_context(|| format!("JSON inválido en la línea {}", n + 1))?;

        if row.status.as_deref() == Some("failed") {
            rep.fallidas += 1;
            let kind = row.metadata.as_ref().and_then(|m| m.kind.as_deref()).unwrap_or("");
            if CURADAS.contains(&kind) {
                let raw = row.content.clone().unwrap_or_default();
                let (body, _) = tags::strip_save_prefix(&raw);
                rep.curated_failed.push((row.id.clone(), title_from(&body)));
            }
            continue;
        }
        let meta = row.metadata.as_ref();
        let kind = meta.and_then(|m| m.kind.as_deref()).unwrap_or("");
        if !CURADAS.contains(&kind) {
            rep.archivadas += 1;
            continue;
        }
        let raw = row.content.unwrap_or_default();
        if raw.trim().is_empty() {
            rep.archivadas += 1;
            continue;
        }

        let (body, save_date) = tags::strip_save_prefix(&raw);
        let created = save_date.unwrap_or_else(|| {
            row.created_at.as_deref().unwrap_or("").chars().take(10).collect()
        });

        let m = NewMemory {
            project: meta.and_then(|m| m.project.clone()).unwrap_or_else(|| "sin-proyecto".into()),
            title: title_from(&body),
            tags: tags::classify(&body).map_err(|e| anyhow::anyhow!(e))?,
            refs: tags::extract_refs(&body),
            body,
            created,
            event_start: None,
            event_end: None,
            source: "migrated".into(),
            ext_id: Some(row.id),
        };

        // ext_id es unique: reinsertar el mismo documento no duplica.
        let ya: i64 = conn.query_row(
            "select count(*) from memories where ext_id = ?1",
            [m.ext_id.as_deref().unwrap()],
            |r| r.get(0),
        )?;
        if ya > 0 {
            rep.migradas += 1;
            continue;
        }
        ingest::insert(conn, &m)?;
        rep.migradas += 1;
    }
    Ok(rep)
}
