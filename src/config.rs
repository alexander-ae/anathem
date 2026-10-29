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
    pub bm25_weight: f64,

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
    pub candidates: usize,

    /// A cuántos caracteres se recorta el fragmento de cada candidato en
    /// `search_memory`, antes de contar su costo contra el presupuesto de
    /// tokens (`search::run_explained`). Ver `candidatos` arriba para el
    /// porqué del recorte.
    #[serde(default = "extracto_por_defecto")]
    pub excerpt: usize,

    /// Cuántas memorias recientes contarle al agente al conectarse, para que
    /// sepa qué dejó el anterior sin tener que preguntar.
    ///
    /// Viaja en el campo `instructions` del `initialize`, que el protocolo
    /// define como una pista que el cliente **puede** sumar al prompt del
    /// sistema. Se usa eso y no un gancho por agente porque es parte de MCP:
    /// funciona en cualquier cliente que lo honre, sin código por herramienta.
    ///
    /// Cuesta tokens en cada sesión, así que es corto y se puede apagar con 0.
    /// Si el proyecto no tuvo actividad en la ventana, no se manda nada.
    #[serde(default = "traspaso_por_defecto")]
    pub traspaso_items: usize,

    /// Qué tan atrás mirar para ese resumen: `6h`, `2d`. Por defecto 24h.
    #[serde(default = "traspaso_ventana_por_defecto")]
    pub traspaso_ventana: String,

    /// Tras cuántos segundos sin buscar, el servidor suelta el paquete de
    /// vocabulario. Con 0 no lo suelta nunca.
    ///
    /// El tokenizador cuesta unos 60 MB de heap privado por proceso. Una
    /// sesión que buscó una vez y se quedó quieta los sigue pagando hasta
    /// cerrarse, y con varios agentes abiertos eso se multiplica. Soltarlo la
    /// devuelve a unos 7 MB; lo único que cuesta es reabrirlo —del orden de
    /// 90 ms— en la próxima búsqueda, que por definición viene después de un
    /// hueco.
    #[serde(default = "release_after_default")]
    pub soltar_paquete_tras: u64,
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

fn traspaso_por_defecto() -> usize {
    5
}

fn traspaso_ventana_por_defecto() -> String {
    "24h".to_string()
}

fn release_after_default() -> u64 {
    300
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bm25_weight: peso_por_defecto(),
            candidates: candidatos_por_defecto(),
            excerpt: extracto_por_defecto(),
            traspaso_items: traspaso_por_defecto(),
            traspaso_ventana: traspaso_ventana_por_defecto(),
            soltar_paquete_tras: release_after_default(),
        }
    }
}

/// Lee `config.json` de `dir`. Si no existe, devuelve los valores por defecto.
/// Si existe y está mal, **falla**: igual que `tags.json`, una configuración
/// rota que se degradara en silencio le haría creer al usuario que su ajuste
/// está aplicado cuando no lo está.
pub fn cargar(dir: &Path) -> Result<Config> {
    let pack_path = dir.join("config.json");
    if !pack_path.exists() {
        return Ok(Config::default());
    }
    let crudo = std::fs::read_to_string(&pack_path)
        .with_context(|| format!("no se pudo leer {}", pack_path.display()))?;
    let c: Config = serde_json::from_str(&crudo)
        .with_context(|| format!("{} no es un config.json válido", pack_path.display()))?;
    if !(c.bm25_weight.is_finite() && c.bm25_weight >= 0.0) {
        bail!(
            "{}: peso_bm25 debe ser un número mayor o igual a cero, no {}",
            pack_path.display(),
            c.bm25_weight
        );
    }
    if c.candidates == 0 {
        bail!("{}: candidatos debe ser mayor que cero, no {}", pack_path.display(), c.candidates);
    }
    if c.excerpt == 0 {
        bail!("{}: extracto debe ser mayor que cero, no {}", pack_path.display(), c.excerpt);
    }
    Ok(c)
}
