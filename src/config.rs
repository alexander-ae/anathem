//! Ajustes que viven en `config.json`, junto a la base de datos.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Cuánto pesa el ranking léxico frente al vectorial al fusionar.
    /// Con 1,0 pesan igual; por debajo de 1,0 manda el vectorial.
    ///
    /// El valor por defecto es 1,0 y se eligió midiendo sobre un corpus real
    /// de memorias, no sobre un banco sintético. Bajarlo a 0,25 —que es lo
    /// que el diseño proponía— cuesta 10 puntos de recall@5 en búsquedas del
    /// mismo idioma (66 % a 56 %) a cambio de 4 puntos entre idiomas. Con 1,0
    /// la búsqueda monolingüe empata con bm25 puro y la cruzada gana 24
    /// puntos. Ver `docs/superpowers/medicion-corpus-real.md`.
    #[serde(default = "peso_por_defecto")]
    pub peso_bm25: f64,

    /// Cuántos candidatos como máximo devuelve `search_memory` por defecto.
    ///
    /// Subirlo de los 10 de antes a 20 apunta a un diseño de dos etapas: la
    /// tool MCP entrega muchos candidatos con extracto corto (ver
    /// `extracto` más abajo) para que el modelo elija entre más opciones
    /// al mismo costo de tokens, y pida el cuerpo completo del que sirve
    /// con `get_memory`. Medido sobre el corpus real del usuario (50
    /// consultas), 20 candidatos de ~120 caracteres cuestan prácticamente
    /// lo mismo en tokens que los ~8 candidatos de fragmento completo de
    /// antes (1.150 contra 1.173), pero la memoria correcta queda entre
    /// las mostradas mucho más seguido (82 % contra 72 %; en consultas
    /// parafrásticas, 50 % contra 25 %).
    #[serde(default = "candidatos_por_defecto")]
    pub candidatos: usize,

    /// A cuántos caracteres se recorta el fragmento de cada candidato en
    /// `search_memory`, antes de contar su costo contra el presupuesto de
    /// tokens (`search::run_explained`). Ver `candidatos` arriba para el
    /// porqué del recorte.
    #[serde(default = "extracto_por_defecto")]
    pub extracto: usize,
}

fn peso_por_defecto() -> f64 {
    1.0
}

fn candidatos_por_defecto() -> usize {
    20
}

fn extracto_por_defecto() -> usize {
    120
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peso_bm25: peso_por_defecto(),
            candidatos: candidatos_por_defecto(),
            extracto: extracto_por_defecto(),
        }
    }
}

/// Lee `config.json` de `dir`. Si no existe, devuelve los valores por defecto.
/// Si existe y está mal, **falla**: igual que `tags.json`, una configuración
/// rota que se degradara en silencio le haría creer al usuario que su ajuste
/// está aplicado cuando no lo está.
pub fn cargar(dir: &Path) -> Result<Config> {
    let ruta = dir.join("config.json");
    if !ruta.exists() {
        return Ok(Config::default());
    }
    let crudo = std::fs::read_to_string(&ruta)
        .with_context(|| format!("no se pudo leer {}", ruta.display()))?;
    let c: Config = serde_json::from_str(&crudo)
        .with_context(|| format!("{} no es un config.json válido", ruta.display()))?;
    if !(c.peso_bm25.is_finite() && c.peso_bm25 >= 0.0) {
        bail!(
            "{}: peso_bm25 debe ser un número mayor o igual a cero, no {}",
            ruta.display(),
            c.peso_bm25
        );
    }
    if c.candidatos == 0 {
        bail!("{}: candidatos debe ser mayor que cero, no {}", ruta.display(), c.candidatos);
    }
    if c.extracto == 0 {
        bail!("{}: extracto debe ser mayor que cero, no {}", ruta.display(), c.extracto);
    }
    Ok(c)
}
