//! Settings that live in `config.json`, next to the database.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// How much the lexical ranking weighs against the vector one when
    /// merging. At 1.0 they weigh the same; below 1.0 the vector side wins.
    ///
    /// The default is 1.0, chosen by measuring on a real corpus of memories
    /// and not on a synthetic benchmark. Lowering it to 0.25 — which is what
    /// the design proposed — costs 10 points of recall@5 on same-language
    /// searches (66% to 56%) in exchange for 4 points across languages. At
    /// 1.0 monolingual search ties with pure bm25 and cross-lingual gains 24
    /// points. See `docs/superpowers/medicion-corpus-real.md`.
    #[serde(default = "peso_por_defecto")]
    pub bm25_weight: f64,

    /// How many candidates `search_memory` returns by default, at most.
    ///
    /// Raising it from the earlier 10 to 20 serves a two-stage design: the
    /// MCP tool hands back many candidates with a short excerpt (see
    /// `excerpt` below) so the model picks among more options at the same
    /// token cost, then asks for the full body of the useful one with
    /// `get_memory`. Measured on the user's real corpus (50 queries), 20
    /// candidates of ~120 characters cost practically the same in tokens as
    /// the earlier ~8 full-snippet candidates (1,150 against 1,173), but the
    /// right memory is among those shown far more often (82% against 72%;
    /// on paraphrastic queries, 50% against 25%).
    #[serde(default = "candidatos_por_defecto")]
    pub candidates: usize,

    /// How many characters each candidate's snippet is trimmed to in
    /// `search_memory`, before its cost counts against the token budget
    /// (`search::run_explained`). See `candidates` above for why it is
    /// trimmed at all.
    #[serde(default = "extracto_por_defecto")]
    pub excerpt: usize,

    /// How many recent memories to tell the agent about on connect, so it
    /// knows what the previous one left behind without having to ask.
    ///
    /// It rides in the `instructions` field of `initialize`, which the
    /// protocol defines as a hint the client **may** add to the system
    /// prompt. We use that instead of a per-agent hook because it is part of
    /// MCP: it works in any client that honours it, with no per-tool code.
    ///
    /// It costs tokens in every session, so it is short and can be turned
    /// off with 0. If the project saw no activity in the window, nothing is
    /// sent at all.
    #[serde(default = "traspaso_por_defecto")]
    pub traspaso_items: usize,

    /// How far back to look for that summary: `6h`, `2d`. Defaults to 24h.
    #[serde(default = "traspaso_ventana_por_defecto")]
    pub traspaso_ventana: String,

    /// After how many seconds without searching the server releases the
    /// vocabulary pack. At 0 it never releases it.
    ///
    /// The tokenizer costs some 60 MB of private heap per process. A session
    /// that searched once and then went quiet keeps paying for it until it
    /// closes, and with several agents open that multiplies. Releasing it
    /// brings the session back to about 7 MB; the only cost is reopening it
    /// — on the order of 90 ms — on the next search, which by definition
    /// comes after a gap.
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

/// Reads `config.json` from `dir`. If it does not exist, returns the
/// defaults. If it exists and is malformed, it **fails**: just like
/// `tags.json`, a broken configuration that degraded silently would let the
/// user believe their setting is applied when it is not.
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
