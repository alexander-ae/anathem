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
    /// points. See `docs/choosing-the-defaults.md`.
    #[serde(default = "default_bm25_weight")]
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
    #[serde(default = "default_candidates")]
    pub candidates: usize,

    /// How many characters each candidate's snippet is trimmed to in
    /// `search_memory`, before its cost counts against the token budget
    /// (`search::run_explained`). See `candidates` above for why it is
    /// trimmed at all.
    #[serde(default = "default_excerpt")]
    pub excerpt: usize,

    /// The tokens `search_memory` spends on its answer when the caller does
    /// not say otherwise.
    ///
    /// It was a constant until it became clear that it silently governs
    /// `candidates`: the budget trims the list before the candidate count
    /// gets to matter, so raising `candidates` to 50 and leaving the budget
    /// at 1,200 changes nothing at all. One of the two was configurable and
    /// the other was not, which made the configurable one a setting that
    /// does nothing on its own.
    ///
    /// A caller may still override it per call, and that override is clamped
    /// rather than rejected — see `search::MAX_BUDGET_TOKENS`.
    #[serde(default = "default_budget")]
    pub budget: usize,

    /// Replaces a long memory's excerpt with a deterministic outline of the
    /// whole memory: its first paragraph and its headings. Off by default.
    ///
    /// The excerpt is a window on one chunk, so a memory with sections is
    /// represented by whichever slice matched and nothing says whether the
    /// rest is relevant. The outline answers that — with no model, nothing
    /// generated and nothing stored, so `mem export` stays reproducible.
    ///
    /// It is off because it costs tokens on every candidate that carries
    /// one, and what it buys is not something `recall@5` can see. See
    /// `docs/benchmarks.md` for the measurement and for what it does and
    /// does not show.
    #[serde(default)]
    pub outline: bool,

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
    #[serde(default = "default_handoff_items")]
    // The key was `traspaso_items` until 0.2.1, when it moved to the field's own
    // English name. The alias is kept so a config.json written before
    // that keeps loading with no warning and no error; it costs one
    // attribute and saves someone their settings on an upgrade.
    #[serde(alias = "traspaso_items")]
    pub handoff_items: usize,

    /// How far back to look for that summary: `6h`, `2d`. Defaults to 24h.
    #[serde(default = "default_handoff_window")]
    // The key was `traspaso_ventana` until 0.2.1, when it moved to the field's own
    // English name. The alias is kept so a config.json written before
    // that keeps loading with no warning and no error; it costs one
    // attribute and saves someone their settings on an upgrade.
    #[serde(alias = "traspaso_ventana")]
    pub handoff_window: String,

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
    // The key was `soltar_paquete_tras` until 0.2.1, when it moved to the field's own
    // English name. The alias is kept so a config.json written before
    // that keeps loading with no warning and no error; it costs one
    // attribute and saves someone their settings on an upgrade.
    #[serde(alias = "soltar_paquete_tras")]
    pub release_pack_after: u64,
}

fn default_bm25_weight() -> f64 {
    1.0
}

fn default_candidates() -> usize {
    20
}

fn default_excerpt() -> usize {
    120
}

fn default_budget() -> usize {
    crate::search::DEFAULT_BUDGET_TOKENS
}

fn default_handoff_items() -> usize {
    5
}

fn default_handoff_window() -> String {
    "24h".to_string()
}

fn release_after_default() -> u64 {
    300
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bm25_weight: default_bm25_weight(),
            candidates: default_candidates(),
            excerpt: default_excerpt(),
            budget: default_budget(),
            outline: false,
            handoff_items: default_handoff_items(),
            handoff_window: default_handoff_window(),
            release_pack_after: release_after_default(),
        }
    }
}

/// Writes one `bm25_weight` into `dir/config.json`, keeping everything else
/// the file already had.
///
/// It edits the parsed JSON object rather than serialising a `Config`,
/// which is the whole point: a `Config` would carry the defaults for the
/// seven keys the file does not mention and write them out as if somebody
/// had chosen them. Anything in there that this version does not know about
/// survives too.
///
/// A file that does not parse is **not** overwritten. Whoever has a broken
/// `config.json` has something in it they meant; losing it to a tuning run
/// would be a poor trade for saving them one edit.
///
/// Returns what happened, so the caller can say it precisely: a file that
/// did not exist is not the same as one that existed without the key.
pub fn write_bm25_weight(dir: &Path, weight: f64) -> Result<Wrote> {
    let ruta = dir.join("config.json");
    let existia = ruta.exists();
    let (mut obj, antes) = if existia {
        let raw = std::fs::read_to_string(&ruta)
            .with_context(|| format!("could not read {}", ruta.display()))?;
        let v: serde_json::Value = serde_json::from_str(&raw).with_context(|| {
            format!(
                "{} is not valid JSON, and it was left untouched rather than                  overwritten: fix it by hand, or move it aside",
                ruta.display()
            )
        })?;
        let obj = match v {
            serde_json::Value::Object(m) => m,
            _ => bail!("{} has to hold a JSON object", ruta.display()),
        };
        let antes = obj.get("bm25_weight").and_then(|v| v.as_f64());
        (obj, antes)
    } else {
        (serde_json::Map::new(), None)
    };
    let n = serde_json::Number::from_f64(weight)
        .ok_or_else(|| anyhow::anyhow!("{weight} cannot be written as JSON"))?;
    obj.insert("bm25_weight".into(), serde_json::Value::Number(n));
    let texto = serde_json::to_string_pretty(&serde_json::Value::Object(obj))?;
    std::fs::write(&ruta, format!("{texto}\n"))
        .with_context(|| format!("could not write {}", ruta.display()))?;
    Ok(match (existia, antes) {
        (false, _) => Wrote::Created,
        (true, None) => Wrote::KeyAdded,
        (true, Some(v)) => Wrote::Replaced(v),
    })
}

/// What `write_bm25_weight` found when it got there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wrote {
    /// There was no `config.json`; one was created holding only this key.
    Created,
    /// The file was there and said nothing about `bm25_weight`.
    KeyAdded,
    /// The file was there and held this value.
    Replaced(f64),
}

/// Reads `config.json` from `dir`. If it does not exist, returns the
/// defaults. If it exists and is malformed, it **fails**: just like
/// `tags.json`, a broken configuration that degraded silently would let the
/// user believe their setting is applied when it is not.
pub fn load(dir: &Path) -> Result<Config> {
    let pack_path = dir.join("config.json");
    if !pack_path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&pack_path)
        .with_context(|| format!("could not read {}", pack_path.display()))?;
    let c: Config = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not a valid config.json", pack_path.display()))?;
    if !(c.bm25_weight.is_finite() && c.bm25_weight >= 0.0) {
        bail!(
            "{}: bm25_weight has to be a number greater than or equal to zero, not {}",
            pack_path.display(),
            c.bm25_weight
        );
    }
    if c.candidates == 0 {
        bail!("{}: candidates has to be greater than zero, not {}", pack_path.display(), c.candidates);
    }
    if c.excerpt == 0 {
        bail!("{}: excerpt has to be greater than zero, not {}", pack_path.display(), c.excerpt);
    }
    if c.budget == 0 {
        bail!("{}: budget has to be greater than zero, not {}", pack_path.display(), c.budget);
    }
    Ok(c)
}
