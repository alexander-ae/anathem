use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Rule {
    tag: &'static str,
    pat: &'static str,
}

/// Taxonomía por defecto: un conjunto pequeño de etiquetas de ingeniería de
/// uso general — sin nombres propios ni vocabulario atado a un stack o
/// equipo particular. Cualquier instalación puede reemplazarla por completo
/// con un archivo `tags.json` junto a la base de datos; ver `load_rules` y
/// el README.
const DEFAULT_RULES: &[Rule] = &[
    Rule { tag: "decision",    pat: r"(?i)\bse decidi[oó]\b|\bse descart[oó]\b|\bse opt[oó] por\b|\btrade-?offs?\b|\bpor qu[eé] se eligi[oó]\b" },
    Rule { tag: "bug",         pat: r"(?i)\bbugs?\b|\berror(es)?\b|\bfalla(s)?\b|\bregresi[oó]n(es)?\b|\bincidente(s)?\b" },
    Rule { tag: "convention",  pat: r"(?i)\bconvenci[oó]n(es)?\b|\best[aá]ndar(es)?\b|\blint\b|\bstyle ?guide\b" },
    Rule { tag: "performance", pat: r"(?i)\bN\+1\b|\b\d+\s*MB\b|\bqueries?\b|\bconsultas?\b|\blatencia\b|\bbenchmark\b|\bbloat\b" },
    Rule { tag: "testing",     pat: r"(?i)\btests?\b|\btesting\b|\bspecs?\b|\bsuites?\b|\bcobertura\b" },
    Rule { tag: "docs",        pat: r"(?i)\bdocs?\b|\bdocumentaci[oó]n\b|\breadme\b|\bchangelog\b" },
    Rule { tag: "code-review", pat: r"(?i)\bPR\s*#\d+|\bpull request\b|\breview(?:er|ó|o)?\b|\brevis(?:ar|é|ó|ando|or|ión|iones)\b" },
];

/// Una regla `{tag, pattern}` tal como viene de un archivo `tags.json` de
/// override.
#[derive(Deserialize)]
struct OverrideRule {
    tag: String,
    pattern: String,
}

pub type Rules = Vec<(String, Regex)>;

fn default_rules() -> Rules {
    DEFAULT_RULES
        .iter()
        .map(|r| (r.tag.to_string(), Regex::new(r.pat).expect("regex de tag por defecto inválida")))
        .collect()
}

/// Compila las reglas de un archivo de override. Falla si el archivo no se
/// puede leer, si no es JSON válido, si está vacío, o si algún `pattern` no
/// compila como regex — en todos los casos el mensaje nombra el archivo (y,
/// para un regex inválido, también la regla ofensora), porque quien escribió
/// un archivo de configuración y ve comportamiento por defecto no tiene
/// forma de notar el error: nunca hay un fallback silencioso a las reglas
/// por defecto.
pub fn load_rules_from_file(path: &Path) -> Result<Rules, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("tags: no se pudo leer {}: {e}", path.display()))?;
    let entries: Vec<OverrideRule> = serde_json::from_str(&raw)
        .map_err(|e| format!("tags: {} no es JSON válido: {e}", path.display()))?;
    if entries.is_empty() {
        return Err(format!("tags: {} no define ninguna regla", path.display()));
    }
    entries
        .into_iter()
        .map(|r| {
            let re = Regex::new(&r.pattern).map_err(|e| {
                format!(
                    "tags: {}: la regla '{}' tiene un patrón inválido: {e}",
                    path.display(),
                    r.tag
                )
            })?;
            Ok((r.tag, re))
        })
        .collect()
}

/// Carga las reglas activas para el directorio `dir` (el directorio de la
/// base de datos): si existe `tags.json` en `dir`, sus reglas *reemplazan
/// por completo* a las por defecto; si no existe (o `dir` es `None`, como en
/// los tests unitarios), se usan las por defecto.
pub fn load_rules(dir: Option<&Path>) -> Result<Rules, String> {
    match dir.map(|d| d.join("tags.json")) {
        Some(path) if path.exists() => load_rules_from_file(&path),
        _ => Ok(default_rules()),
    }
}

/// Directorio donde `classify`/`extract_refs` buscarán `tags.json`. Se fija
/// una sola vez, con `init`, antes del primer uso de `classify`; llamadas
/// posteriores a `init` no tienen efecto. Si nunca se llama (como en los
/// tests) el directorio queda `None` y se usan las reglas por defecto.
static DB_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Fija el directorio de la base de datos activa para esta ejecución del
/// binario. Debe llamarse, si se va a llamar, antes de la primera llamada a
/// `classify`.
pub fn init(db_path: &Path) {
    let _ = DB_DIR.set(db_path.parent().map(|p| p.to_path_buf()));
}

fn active() -> Result<&'static Rules, String> {
    static ACTIVE: OnceLock<Result<Rules, String>> = OnceLock::new();
    ACTIVE
        .get_or_init(|| {
            let dir = DB_DIR.get().cloned().flatten();
            load_rules(dir.as_deref())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Clasifica `body` contra las reglas activas (por defecto, o las de
/// `tags.json` si hay una instalada — ver `init`/`load_rules`).
pub fn classify(body: &str) -> Result<Vec<String>, String> {
    let rules = active()?;
    Ok(rules
        .iter()
        .filter(|(_, re)| re.is_match(body))
        .map(|(t, _)| t.clone())
        .collect())
}

fn re_ticket() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Convención genérica de ticket ID (Jira, Linear, GitHub Projects, ...):
    // un prefijo de proyecto en mayúsculas seguido de un número. No asume
    // ningún prefijo particular.
    R.get_or_init(|| Regex::new(r"\b[A-Z]{2,10}-\d+\b").unwrap())
}
fn re_pr() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bPR\s*#(\d+)\b").unwrap())
}
fn re_sha() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Match 8-40 hex characters; validate at least one [a-f] in extract_refs.
    // This avoids lookarounds which the regex crate doesn't support.
    R.get_or_init(|| Regex::new(r"\b[0-9a-f]{8,40}\b").unwrap())
}

pub fn extract_refs(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in re_ticket().find_iter(body) {
        out.push(("ticket".into(), m.as_str().to_string()));
    }
    for c in re_pr().captures_iter(body) {
        out.push(("pr".into(), c[1].to_string()));
    }
    for m in re_sha().find_iter(body) {
        let s = m.as_str();
        // Validate that it contains at least one hex letter (a-f) to avoid matching pure decimal numbers.
        if s.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
            out.push(("sha".into(), s.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn re_save() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[SAVE:[^:\]]+:(\d{4}-\d{2}-\d{2})\]\s*").unwrap())
}

fn re_save_end() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // El terminador puede venir precedido de espacio en blanco (incluyendo el
    // salto de línea que normalmente lo separa del cuerpo); lo consumimos
    // junto con el marcador para no dejar una línea en blanco colgando.
    R.get_or_init(|| Regex::new(r"\s*\[/SAVE\]\s*$").unwrap())
}

/// Saca el marcador `[SAVE:proyecto:fecha]` de apertura y el `[/SAVE]` de
/// cierre. El de apertura se removió desde el principio; el de cierre no —
/// sobrevivía en el 87% de los cuerpos migrados y se colaba en snippets de
/// búsqueda del MCP, en los archivos exportados y en el índice FTS.
pub fn strip_save_prefix(body: &str) -> (String, Option<String>) {
    let (rest, date) = match re_save().captures(body) {
        Some(c) => {
            let date = c.get(1).map(|m| m.as_str().to_string());
            let rest = re_save().replace(body, "").trim_start().to_string();
            (rest, date)
        }
        None => (body.to_string(), None),
    };
    let rest = re_save_end().replace(&rest, "").trim_end().to_string();
    (rest, date)
}
