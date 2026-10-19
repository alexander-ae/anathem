//! Fusión de dos rankings por Reciprocal Rank Fusion.
//!
//! Función pura: recibe dos listas de `memory_id` ya ordenadas de mejor a peor
//! y devuelve una sola. No toca la base ni el modelo, para que la decisión de
//! orden se pueda probar con listas escritas a mano.

use std::cmp::Ordering;
use std::collections::HashMap;

/// Constante de amortiguación de RRF. El valor 60 es el del paper original y
/// el que se usó en todas las mediciones del diseño.
pub const K: f64 = 60.0;

/// Fusiona `bm25` y `vectorial`. `peso_bm25` escala el aporte del ranking
/// léxico: con 1,0 los dos pesan igual, por debajo de 1,0 el vectorial manda.
///
/// Sobre el valor por defecto de 1,0 y por qué no hay uno que gane en las dos
/// direcciones, ver `docs/superpowers/medicion-corpus-real.md`.
pub fn rrf(bm25: &[i64], vectorial: &[i64], peso_bm25: f64) -> Vec<i64> {
    let mut punto: HashMap<i64, f64> = HashMap::new();
    for (i, id) in bm25.iter().enumerate() {
        *punto.entry(*id).or_insert(0.0) += peso_bm25 / (K + (i + 1) as f64);
    }
    for (i, id) in vectorial.iter().enumerate() {
        *punto.entry(*id).or_insert(0.0) += 1.0 / (K + (i + 1) as f64);
    }
    let mut ids: Vec<(i64, f64)> = punto.into_iter().collect();
    // Mayor puntaje primero; a igual puntaje, menor id. El desempate por id no
    // es cosmético: `punto` es un HashMap y sin él dos documentos empatados
    // saldrían en orden distinto entre corridas idénticas.
    ids.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal).then(a.0.cmp(&b.0))
    });
    ids.into_iter().map(|(id, _)| id).collect()
}
