#[derive(Debug, Clone)]
pub struct NewMemory {
    pub project: String,
    pub title: String,
    pub body: String,
    pub created: String,
    pub event_start: Option<String>,
    pub event_end: Option<String>,
    pub source: String,
    pub ext_id: Option<String>,
    pub tags: Vec<String>,
    /// Pares (kind, value) donde kind es "ticket", "pr" o "sha".
    pub refs: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub memory_id: i64,
    pub ext_id: Option<String>,
    pub project: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}
