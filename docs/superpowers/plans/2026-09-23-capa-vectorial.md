# Capa vectorial opcional — Plan de implementación

> **Para trabajadores agénticos:** SUB-SKILL REQUERIDA: usar
> superpowers:subagent-driven-development (recomendado) o
> superpowers:executing-plans para implementar tarea por tarea. Los pasos usan
> casillas (`- [ ]`) para seguimiento.

**Meta:** que una memoria escrita en un idioma se encuentre preguntando en
otro, sin que quien no lo necesite pague nada.

**Arquitectura:** un paquete de vocabulario (modelo estático recortado y
cuantizado a int8, 22,5 MB) se mapea en memoria y convierte texto en vectores
de 256 dimensiones. Los vectores de cada fragmento se guardan en SQLite y se
recorren por fuerza bruta. El orden final sale de fusionar por RRF el ranking
de BM25 con el vectorial. Sin paquete instalado no se ejecuta nada de esto.

**Stack:** Rust 2021, rusqlite (bundled), tokenizers 0.20, memmap2, sha2.

**Especificación:** `docs/superpowers/specs/2026-09-23-capa-vectorial-design.md`
**Formato del paquete:** `docs/paquetes-de-vocabulario.md` (normativo)

## Restricciones globales

- Dependencias nuevas, exactamente estas y ninguna más:
  `tokenizers = { version = "0.20", default-features = false, features = ["onig"] }`,
  `memmap2 = "0.9"`, `sha2 = "0.10"`.
- Prohibido agregar: onnxruntime, torch, openssl, reqwest, sqlite-vec.
- El proyecto ya compila C vía `rusqlite` con `bundled`; `onig` no agrega una
  exigencia de otra clase. (`tokenizers` 1.0 elimina esa dependencia pero
  reestructuró la API y está en release candidate: no se usa todavía.)
- Comentarios, mensajes al usuario y nombres de test **en español**, como el
  resto del repositorio.
- Constantes fijas: `fusion::K = 60.0`, `config::Config::default().peso_bm25 = 0.25`.
- Formato del paquete: little endian, marca `ANPK`, versión 1.
- **Sin paquete instalado, el comportamiento debe ser idéntico al actual.** La
  batería de tests existente tiene que seguir pasando sin cambios de
  expectativas.
- Cada tarea termina con `cargo test` en verde y un commit.
- Los commits terminan con la línea de coautoría del `CLAUDE.md` del usuario.

---

### Task 1: Leer un paquete y embeber texto

**Archivos:**
- Crear: `src/pack.rs`
- Crear: `tests/apoyo/mod.rs`
- Crear: `tests/pack_test.rs`
- Modificar: `src/lib.rs`, `Cargo.toml`

**Interfaces:**
- Consume: nada.
- Produce:
  - `pack::Pack` con `Pack::abrir(&Path) -> Result<Pack>`,
    `.nombre() -> &str`, `.dims() -> usize`,
    `.embeber(&str) -> Result<Vec<f32>>` (vector normalizado).
  - `pack::sha256(&Path) -> Result<String>` (hexadecimal en minúsculas).
  - `apoyo::escribir_pack_mini(&Path, nombre: &str)` para los tests.

- [ ] **Paso 1: agregar dependencias**

En `Cargo.toml`, dentro de `[dependencies]`:

```toml
memmap2 = "0.9"
sha2 = "0.10"
tokenizers = { version = "0.20", default-features = false, features = ["onig"] }
```

- [ ] **Paso 2: escribir el ayudante de fixture**

Crear `tests/apoyo/mod.rs`:

```rust
use std::path::Path;

/// Tokenizador Unigram mínimo. Cuatro piezas: el desconocido, dos palabras y
/// un sufijo. El sufijo está para comprobar la re-segmentación: "mundos" se
/// parte en "▁mundo" + "s" en vez de caer en desconocido.
pub const TOKENIZER_MINI: &str = r#"{
 "version":"1.0","truncation":null,"padding":null,"added_tokens":[],
 "normalizer":null,
 "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
 "post_processor":null,"decoder":null,
 "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
   "vocab":[["<unk>",0.0],["▁hola",-1.0],["▁mundo",-2.0],["s",-8.0]]}
}"#;

/// Escribe un paquete de 4 tokens y 4 dimensiones en `ruta`.
///
/// La escala es 1,0 y el mínimo 0,0 en cada dimensión, así el valor
/// descuantizado es el byte tal cual y los vectores esperados se calculan a
/// mano. Filas: `<unk>`=[0,0,0,0], `▁hola`=[10,0,0,0], `▁mundo`=[0,10,0,0],
/// `s`=[0,0,10,0].
pub fn escribir_pack_mini(ruta: &Path, nombre: &str) {
    let tok = TOKENIZER_MINI.as_bytes();
    let dims = 4usize;
    let filas: [[u8; 4]; 4] = [[0, 0, 0, 0], [10, 0, 0, 0], [0, 10, 0, 0], [0, 0, 10, 0]];
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(filas.len() as u32).to_le_bytes());
    b.extend_from_slice(&(nombre.len() as u32).to_le_bytes());
    b.extend_from_slice(nombre.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims {
        b.extend_from_slice(&1.0f32.to_le_bytes());
    }
    for _ in 0..dims {
        b.extend_from_slice(&0.0f32.to_le_bytes());
    }
    for f in filas {
        b.extend_from_slice(&f);
    }
    std::fs::write(ruta, b).unwrap();
}

/// Reescribe un byte suelto del archivo, para construir paquetes corruptos.
pub fn corromper(ruta: &Path, offset: usize, valor: u8) {
    let mut b = std::fs::read(ruta).unwrap();
    b[offset] = valor;
    std::fs::write(ruta, b).unwrap();
}
```

- [ ] **Paso 3: escribir los tests que fallan**

Crear `tests/pack_test.rs`:

```rust
mod apoyo;

use anathem::pack::Pack;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn abre_y_reporta_sus_metadatos() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    assert_eq!(p.nombre(), "mini-es");
    assert_eq!(p.dims(), 4);
}

#[test]
fn un_solo_token_da_su_eje() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    let v = p.embeber("hola").unwrap();
    assert!((v[0] - 1.0).abs() < 1e-5, "esperaba el eje 0, salió {v:?}");
    assert!(v[1].abs() < 1e-5 && v[2].abs() < 1e-5 && v[3].abs() < 1e-5);
}

#[test]
fn dos_tokens_promedian_sus_filas() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    let v = p.embeber("hola mundo").unwrap();
    let esperado = 1.0f32 / 2f32.sqrt();
    assert!((v[0] - esperado).abs() < 1e-5, "salió {v:?}");
    assert!((v[1] - esperado).abs() < 1e-5, "salió {v:?}");
}

#[test]
fn el_sufijo_se_resegmenta_en_vez_de_caer_en_desconocido() {
    // "mundos" no es una pieza, pero "▁mundo" y "s" sí. Esta es la propiedad
    // que hace que recortar el vocabulario no pierda calidad.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    let v = p.embeber("mundos").unwrap();
    assert!(v[1] > 0.1 && v[2] > 0.1, "esperaba mezcla de ▁mundo y s, salió {v:?}");
}

#[test]
fn texto_vacio_da_un_vector_nulo_sin_fallar() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    let v = p.embeber("").unwrap();
    assert_eq!(v.len(), 4);
    assert!(cos(&v, &v) < 1e-9, "esperaba el vector nulo, salió {v:?}");
}

#[test]
fn rechaza_un_archivo_sin_la_marca() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("basura.anathem-pack");
    std::fs::write(&r, b"esto no es un paquete en absoluto").unwrap();
    let e = Pack::abrir(&r).unwrap_err().to_string();
    assert!(e.contains("ANPK"), "el error debe nombrar la marca: {e}");
}

#[test]
fn rechaza_una_version_que_no_entiende() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    apoyo::corromper(&r, 4, 99); // el primer byte de la versión
    let e = Pack::abrir(&r).unwrap_err().to_string();
    assert!(e.contains("versión"), "el error debe nombrar la versión: {e}");
}

#[test]
fn rechaza_un_archivo_truncado() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let b = std::fs::read(&r).unwrap();
    std::fs::write(&r, &b[..b.len() - 5]).unwrap();
    assert!(Pack::abrir(&r).is_err(), "un paquete truncado no puede abrirse");
}

#[test]
fn el_sha256_es_estable_y_cambia_con_el_contenido() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let a = anathem::pack::sha256(&r).unwrap();
    assert_eq!(a, anathem::pack::sha256(&r).unwrap());
    assert_eq!(a.len(), 64);
    apoyo::corromper(&r, 40, 7);
    assert_ne!(a, anathem::pack::sha256(&r).unwrap());
}
```

- [ ] **Paso 4: correr los tests y verificar que fallan**

Correr: `cargo test --test pack_test`
Esperado: no compila, `unresolved import anathem::pack`.

- [ ] **Paso 5: escribir `src/pack.rs`**

```rust
//! Lectura de un paquete de vocabulario y conversión de texto a vectores.
//!
//! El formato está documentado en `docs/paquetes-de-vocabulario.md` y lo
//! genera `tools/construir_paquete.py`. Este módulo es lo único del binario
//! que sabe que existe un tokenizador o una cuantización.

use anyhow::{bail, Context, Result};
use memmap2::Mmap;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokenizers::Tokenizer;

const MARCA: &[u8; 4] = b"ANPK";
const VERSION: u32 = 1;

pub struct Pack {
    mapa: Mmap,
    tok: Tokenizer,
    nombre: String,
    dims: usize,
    n_tokens: usize,
    escalas: Vec<f32>,
    minimos: Vec<f32>,
    inicio_tabla: usize,
}

fn u32_en(m: &[u8], o: usize) -> Result<u32> {
    let f = m.get(o..o + 4).context("el paquete termina antes de lo que declara")?;
    Ok(u32::from_le_bytes(f.try_into().unwrap()))
}

fn u64_en(m: &[u8], o: usize) -> Result<u64> {
    let f = m.get(o..o + 8).context("el paquete termina antes de lo que declara")?;
    Ok(u64::from_le_bytes(f.try_into().unwrap()))
}

fn f32s_en(m: &[u8], o: usize, n: usize) -> Result<Vec<f32>> {
    let f = m.get(o..o + 4 * n).context("el paquete termina antes de lo que declara")?;
    Ok(f.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}

impl Pack {
    /// Abre y valida un paquete. **No** verifica el sha256: hacerlo en cada
    /// arranque costaría hashear decenas de megabytes y penalizaría a quien
    /// ni siquiera usa la capa vectorial. El sha se verifica una sola vez, al
    /// instalar (ver `mem model install`).
    pub fn abrir(ruta: &Path) -> Result<Pack> {
        let archivo = std::fs::File::open(ruta)
            .with_context(|| format!("no se pudo abrir el paquete {}", ruta.display()))?;
        let mapa = unsafe { Mmap::map(&archivo)? };
        if mapa.len() < 20 || &mapa[0..4] != MARCA {
            bail!("{} no es un paquete de anathem: falta la marca ANPK", ruta.display());
        }
        let version = u32_en(&mapa, 4)?;
        if version != VERSION {
            bail!(
                "el paquete {} declara la versión {version} y este binario entiende la {VERSION}",
                ruta.display()
            );
        }
        let dims = u32_en(&mapa, 8)? as usize;
        let n_tokens = u32_en(&mapa, 12)? as usize;
        let largo_nombre = u32_en(&mapa, 16)? as usize;
        if dims == 0 || n_tokens == 0 {
            bail!("el paquete {} declara 0 dimensiones o 0 tokens", ruta.display());
        }
        let mut o = 20;
        let nombre = std::str::from_utf8(
            mapa.get(o..o + largo_nombre).context("nombre del paquete truncado")?,
        )?
        .to_string();
        o += largo_nombre;
        let largo_tok = u64_en(&mapa, o)? as usize;
        o += 8;
        let crudo = mapa.get(o..o + largo_tok).context("tokenizador truncado")?;
        let tok = Tokenizer::from_bytes(crudo)
            .map_err(|e| anyhow::anyhow!("el tokenizador del paquete no se pudo leer: {e}"))?;
        o += largo_tok;
        let escalas = f32s_en(&mapa, o, dims)?;
        o += 4 * dims;
        let minimos = f32s_en(&mapa, o, dims)?;
        o += 4 * dims;
        if mapa.len() < o + n_tokens * dims {
            bail!("la tabla del paquete {} está truncada", ruta.display());
        }
        Ok(Pack { mapa, tok, nombre, dims, n_tokens, escalas, minimos, inicio_tabla: o })
    }

    pub fn nombre(&self) -> &str {
        &self.nombre
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    /// Promedio de las filas de los tokens del texto, normalizado.
    /// Un texto sin tokens devuelve el vector nulo en vez de fallar: es una
    /// entrada legítima (un fragmento de solo espacios) y no un error.
    pub fn embeber(&self, texto: &str) -> Result<Vec<f32>> {
        let enc = self
            .tok
            .encode(texto, false)
            .map_err(|e| anyhow::anyhow!("no se pudo tokenizar: {e}"))?;
        let mut v = vec![0f32; self.dims];
        let mut usados = 0usize;
        for &id in enc.get_ids() {
            let i = id as usize;
            if i >= self.n_tokens {
                continue;
            }
            let a = self.inicio_tabla + i * self.dims;
            let fila = &self.mapa[a..a + self.dims];
            for d in 0..self.dims {
                v[d] += fila[d] as f32 * self.escalas[d] + self.minimos[d];
            }
            usados += 1;
        }
        if usados == 0 {
            return Ok(v);
        }
        let k = usados as f32;
        let mut norma = 0f32;
        for d in 0..self.dims {
            v[d] /= k;
            norma += v[d] * v[d];
        }
        let norma = norma.sqrt();
        if norma > 1e-9 {
            for d in 0..self.dims {
                v[d] /= norma;
            }
        }
        Ok(v)
    }
}

/// sha256 del archivo completo, en hexadecimal minúsculo.
pub fn sha256(ruta: &Path) -> Result<String> {
    let mut f = std::fs::File::open(ruta)
        .with_context(|| format!("no se pudo abrir {}", ruta.display()))?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}
```

Agregar en `src/lib.rs`, en orden alfabético: `pub mod pack;`

- [ ] **Paso 6: correr los tests y verificar que pasan**

Correr: `cargo test --test pack_test`
Esperado: 9 tests PASS.

- [ ] **Paso 7: commit**

```bash
git add Cargo.toml Cargo.lock src/pack.rs src/lib.rs tests/apoyo/mod.rs tests/pack_test.rs
git commit -m "feat: leer paquetes de vocabulario y embeber texto"
```

---

### Task 2: Fusión RRF ponderada

**Archivos:**
- Crear: `src/fusion.rs`
- Crear: `tests/fusion_test.rs`
- Modificar: `src/lib.rs`

**Interfaces:**
- Consume: nada. Función pura sobre listas de enteros.
- Produce: `fusion::K: f64` (60,0) y
  `fusion::rrf(bm25: &[i64], vectorial: &[i64], peso_bm25: f64) -> Vec<i64>`.

Va en su propio módulo, sin base de datos y sin modelo, porque acá vive el
compromiso entre búsqueda monolingüe y cruzada: es la decisión más discutible
del diseño y tiene que poder auditarse con listas escritas a mano.

- [ ] **Paso 1: escribir los tests que fallan**

Crear `tests/fusion_test.rs`:

```rust
use anathem::fusion::rrf;

#[test]
fn sin_ranking_vectorial_conserva_el_de_bm25() {
    assert_eq!(rrf(&[10, 20, 30], &[], 0.25), vec![10, 20, 30]);
}

#[test]
fn sin_ranking_bm25_conserva_el_vectorial() {
    assert_eq!(rrf(&[], &[30, 20, 10], 0.25), vec![30, 20, 10]);
}

#[test]
fn con_peso_bajo_el_vectorial_le_gana_a_bm25() {
    // Cada uno aporta un documento distinto en la posición 1. Con peso 0,25
    // el de bm25 vale la cuarta parte, así que manda el vectorial.
    assert_eq!(rrf(&[10], &[20], 0.25), vec![20, 10]);
}

#[test]
fn con_peso_uno_empatan_y_desempata_el_id() {
    // Mismo caso, pesos iguales: los puntajes son idénticos y el desempate
    // estable por id deja primero al menor. Sin ese desempate el orden
    // dependería del recorrido del HashMap y variaría entre corridas.
    assert_eq!(rrf(&[10], &[20], 1.0), vec![10, 20]);
}

#[test]
fn aparecer_en_los_dos_rankings_pesa_mas_que_liderar_uno() {
    // 20 está en ambos (posición 2 en bm25, 1 en vectorial); 30 solo en el
    // vectorial en posición 2; 10 solo en bm25 en posición 1.
    assert_eq!(rrf(&[10, 20], &[20, 30], 0.25), vec![20, 30, 10]);
}

#[test]
fn no_repite_documentos() {
    let r = rrf(&[1, 2, 3], &[3, 2, 1], 0.5);
    let mut u = r.clone();
    u.sort_unstable();
    u.dedup();
    assert_eq!(r.len(), u.len(), "hay duplicados en {r:?}");
    assert_eq!(u, vec![1, 2, 3]);
}

#[test]
fn es_determinista() {
    let a = rrf(&[5, 4, 3, 2, 1], &[1, 2, 3, 4, 5], 0.25);
    for _ in 0..20 {
        assert_eq!(a, rrf(&[5, 4, 3, 2, 1], &[1, 2, 3, 4, 5], 0.25));
    }
}

#[test]
fn con_ambos_vacios_devuelve_vacio() {
    assert!(rrf(&[], &[], 0.25).is_empty());
}
```

- [ ] **Paso 2: correr los tests y verificar que fallan**

Correr: `cargo test --test fusion_test`
Esperado: no compila, `unresolved import anathem::fusion`.

- [ ] **Paso 3: escribir `src/fusion.rs`**

```rust
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
/// léxico: con 1,0 los dos pesan igual, con 0,25 el vectorial manda.
///
/// Sobre el valor por defecto de 0,25 y por qué no hay uno que gane en las dos
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
```

Agregar en `src/lib.rs`: `pub mod fusion;`

- [ ] **Paso 4: correr los tests y verificar que pasan**

Correr: `cargo test --test fusion_test`
Esperado: 8 tests PASS.

- [ ] **Paso 5: commit**

```bash
git add src/fusion.rs src/lib.rs tests/fusion_test.rs
git commit -m "feat: fusion RRF ponderada entre bm25 y vectorial"
```

---

### Task 3: Esquema para vectores y metadatos

**Archivos:**
- Modificar: `src/db.rs`
- Modificar: `tests/db_test.rs`

**Interfaces:**
- Consume: nada.
- Produce: tablas `chunk_vectors(chunk_id, vec)` y `meta(clave, valor)`.
  `chunk_vectors.chunk_id` referencia `chunks(id)` con `on delete cascade`.

El cascade importa: `edit::rechunk` borra y reinserta los fragmentos de una
memoria al editarla o al reindexar. Con el cascade, los vectores viejos se van
solos y la sincronización posterior los reconstruye. Sin él quedarían
apuntando a fragmentos que ya no existen.

- [ ] **Paso 1: escribir el test que falla**

Agregar al final de `tests/db_test.rs`:

```rust
#[test]
fn borrar_una_memoria_se_lleva_sus_vectores() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = ingest::insert(&conn, &NewMemory {
        project: "p".into(),
        title: "t".into(),
        body: "un cuerpo cualquiera que produce al menos un fragmento".into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();
    let chunk: i64 = conn
        .query_row("select id from chunks where memory_id=?1", [id], |r| r.get(0))
        .unwrap();
    conn.execute("insert into chunk_vectors (chunk_id, vec) values (?1, ?2)",
                 rusqlite::params![chunk, vec![0u8; 16]]).unwrap();

    conn.execute("delete from memories where id=?1", [id]).unwrap();

    let quedan: i64 = conn
        .query_row("select count(*) from chunk_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(quedan, 0, "el cascade debe llevarse los vectores");
}

#[test]
fn meta_guarda_pares_clave_valor() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute("insert into meta (clave, valor) values ('pack_nombre','en-es')", []).unwrap();
    let v: String = conn
        .query_row("select valor from meta where clave='pack_nombre'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "en-es");
}
```

Si `tests/db_test.rs` no importa ya `NewMemory`, `ingest` o `rusqlite`,
agregarlos al `use` de arriba del archivo.

- [ ] **Paso 2: correr los tests y verificar que fallan**

Correr: `cargo test --test db_test`
Esperado: FAIL con `no such table: chunk_vectors`.

- [ ] **Paso 3: agregar las tablas al esquema**

En `src/db.rs`, dentro de la constante `SCHEMA`, después de la definición de
`chunks_fts` y antes de `search_log`:

```sql
create table if not exists chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);

create table if not exists meta (
  clave text primary key,
  valor text not null
);
```

- [ ] **Paso 4: correr los tests y verificar que pasan**

Correr: `cargo test --test db_test`
Esperado: todos PASS, incluidos los dos nuevos.

- [ ] **Paso 5: correr la batería completa**

Correr: `cargo test`
Esperado: todo verde. Las tablas nuevas son aditivas y no cambian nada
existente.

- [ ] **Paso 6: commit**

```bash
git add src/db.rs tests/db_test.rs
git commit -m "feat: tablas chunk_vectors y meta"
```

---

### Task 4: Guardar, sincronizar y buscar vectores

**Archivos:**
- Crear: `src/vecstore.rs`
- Crear: `tests/vecstore_test.rs`
- Modificar: `src/lib.rs`

**Interfaces:**
- Consume: `pack::Pack` (Task 1), tablas de Task 3, `search::Query` (ya existe).
- Produce:
  - `vecstore::Acierto { memory_id: i64, chunk_id: i64, coseno: f32 }`
  - `vecstore::registrar_pack(&Connection, nombre: &str, sha: &str, dims: usize) -> Result<()>`
  - `vecstore::pack_registrado(&Connection) -> Result<Option<(String, String, usize)>>`
    (nombre, sha256, dims)
  - `vecstore::olvidar_pack(&Connection) -> Result<()>`
  - `vecstore::sincronizar(&Connection, &Pack) -> Result<usize>` (devuelve
    cuántos fragmentos embebió; idempotente)
  - `vecstore::cobertura(&Connection) -> Result<(usize, usize)>` (con vector, total)
  - `vecstore::buscar(&Connection, &[f32], &search::Query, usize) -> Result<Vec<Acierto>>`

`sincronizar` es idempotente a propósito: en vez de embeber dentro de
`ingest::insert` y de `edit::replace_body` y de `migrate` —tres lugares que
pueden divergir, como ya pasó con el guardado explícito— hay una sola función
que llena los huecos, y los caminos de escritura la llaman al terminar.

- [ ] **Paso 1: escribir los tests que fallan**

Crear `tests/vecstore_test.rs`:

```rust
mod apoyo;

use anathem::{db, ingest, model::NewMemory, pack::Pack, search, vecstore};
use rusqlite::Connection;

fn sembrar(conn: &Connection, cuerpo: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: cuerpo.chars().take(30).collect(),
        body: cuerpo.into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap()
}

fn fixture() -> (tempfile::TempDir, Connection, Pack) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = Pack::abrir(&r).unwrap();
    (dir, conn, p)
}

#[test]
fn sincronizar_llena_los_huecos_y_es_idempotente() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    sembrar(&conn, "mundo");
    assert_eq!(vecstore::sincronizar(&conn, &p).unwrap(), 2);
    assert_eq!(vecstore::sincronizar(&conn, &p).unwrap(), 0, "no debe reembeber");
    assert_eq!(vecstore::cobertura(&conn).unwrap(), (2, 2));
}

#[test]
fn cobertura_muestra_el_hueco_de_lo_guardado_sin_paquete() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    vecstore::sincronizar(&conn, &p).unwrap();
    sembrar(&conn, "mundo"); // guardada después, sin sincronizar
    assert_eq!(vecstore::cobertura(&conn).unwrap(), (1, 2));
}

#[test]
fn buscar_ordena_por_coseno() {
    let (_d, conn, p) = fixture();
    let hola = sembrar(&conn, "hola");
    let mundo = sembrar(&conn, "mundo");
    vecstore::sincronizar(&conn, &p).unwrap();
    let q = search::Query { text: String::new(), ..Default::default() };
    let v = p.embeber("hola").unwrap();
    let r = vecstore::buscar(&conn, &v, &q, 10).unwrap();
    assert_eq!(r[0].memory_id, hola, "la más parecida debe ir primera");
    assert!(r[0].coseno > r[1].coseno);
    assert!(r.iter().any(|a| a.memory_id == mundo));
}

#[test]
fn buscar_respeta_el_filtro_de_proyecto() {
    let (_d, conn, p) = fixture();
    let a = ingest::insert(&conn, &NewMemory {
        project: "uno".into(), title: "t".into(), body: "hola".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "dos".into(), title: "t".into(), body: "hola".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    vecstore::sincronizar(&conn, &p).unwrap();
    let q = search::Query { project: Some("uno".into()), ..Default::default() };
    let r = vecstore::buscar(&conn, &p.embeber("hola").unwrap(), &q, 10).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].memory_id, a);
}

#[test]
fn buscar_ignora_las_memorias_no_activas() {
    let (_d, conn, p) = fixture();
    let id = sembrar(&conn, "hola");
    vecstore::sincronizar(&conn, &p).unwrap();
    conn.execute("update memories set status='forgotten' where id=?1", [id]).unwrap();
    let r = vecstore::buscar(&conn, &p.embeber("hola").unwrap(), &search::Query::default(), 10).unwrap();
    assert!(r.is_empty(), "una memoria olvidada no puede salir en la búsqueda vectorial");
}

#[test]
fn el_registro_del_paquete_va_y_vuelve() {
    let (_d, conn, _p) = fixture();
    assert!(vecstore::pack_registrado(&conn).unwrap().is_none());
    vecstore::registrar_pack(&conn, "en-es", "abc123", 256).unwrap();
    assert_eq!(
        vecstore::pack_registrado(&conn).unwrap(),
        Some(("en-es".into(), "abc123".into(), 256))
    );
    // registrar de nuevo pisa, no duplica
    vecstore::registrar_pack(&conn, "en-es-pt", "def456", 256).unwrap();
    assert_eq!(
        vecstore::pack_registrado(&conn).unwrap(),
        Some(("en-es-pt".into(), "def456".into(), 256))
    );
}

#[test]
fn olvidar_el_paquete_borra_registro_y_vectores() {
    let (_d, conn, p) = fixture();
    sembrar(&conn, "hola");
    vecstore::sincronizar(&conn, &p).unwrap();
    vecstore::registrar_pack(&conn, "mini-es", "abc", 4).unwrap();
    vecstore::olvidar_pack(&conn).unwrap();
    assert!(vecstore::pack_registrado(&conn).unwrap().is_none());
    assert_eq!(vecstore::cobertura(&conn).unwrap(), (0, 1));
}

#[test]
fn editar_una_memoria_invalida_sus_vectores() {
    // rechunk borra los fragmentos; el cascade se lleva los vectores y
    // sincronizar los reconstruye. Sin esto, una memoria editada quedaría
    // indexada por su texto viejo.
    let (_d, conn, p) = fixture();
    let id = sembrar(&conn, "hola");
    vecstore::sincronizar(&conn, &p).unwrap();
    anathem::edit::replace_body(&conn, id, "mundo").unwrap();
    assert_eq!(vecstore::cobertura(&conn).unwrap().0, 0, "los vectores viejos deben irse");
    assert_eq!(vecstore::sincronizar(&conn, &p).unwrap(), 1);
}
```

- [ ] **Paso 2: correr los tests y verificar que fallan**

Correr: `cargo test --test vecstore_test`
Esperado: no compila, `unresolved import anathem::vecstore`.

- [ ] **Paso 3: escribir `src/vecstore.rs`**

```rust
//! Persistencia de los vectores y búsqueda por coseno.
//!
//! La búsqueda es por fuerza bruta, sin índice aproximado y sin extensiones de
//! SQLite. Un producto punto contra 100.000 fragmentos son 25 millones de
//! operaciones, unos pocos milisegundos, y evita una extensión compilada que
//! rompería la promesa de un solo binario. Conviene revisarlo por encima del
//! medio millón de fragmentos.

use crate::{pack::Pack, search::Query};
use anyhow::Result;
use rusqlite::{params, types::Value, Connection};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Acierto {
    pub memory_id: i64,
    pub chunk_id: i64,
    pub coseno: f32,
}

fn a_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn de_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

pub fn registrar_pack(conn: &Connection, nombre: &str, sha: &str, dims: usize) -> Result<()> {
    for (k, v) in [
        ("pack_nombre", nombre.to_string()),
        ("pack_sha256", sha.to_string()),
        ("pack_dims", dims.to_string()),
    ] {
        conn.execute(
            "insert into meta (clave, valor) values (?1, ?2)
             on conflict(clave) do update set valor = ?2",
            params![k, v],
        )?;
    }
    Ok(())
}

pub fn pack_registrado(conn: &Connection) -> Result<Option<(String, String, usize)>> {
    let leer = |k: &str| -> Result<Option<String>> {
        Ok(conn
            .query_row("select valor from meta where clave = ?1", [k], |r| r.get::<_, String>(0))
            .ok())
    };
    match (leer("pack_nombre")?, leer("pack_sha256")?, leer("pack_dims")?) {
        (Some(n), Some(s), Some(d)) => Ok(Some((n, s, d.parse()?))),
        _ => Ok(None),
    }
}

pub fn olvidar_pack(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("delete from chunk_vectors", [])?;
    tx.execute(
        "delete from meta where clave in ('pack_nombre','pack_sha256','pack_dims')",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Embebe los fragmentos que todavía no tienen vector y devuelve cuántos.
///
/// Es idempotente a propósito: los caminos de escritura (guardar, editar,
/// migrar, reindexar) la llaman al terminar en vez de embeber cada uno por su
/// cuenta, para que no puedan divergir entre sí.
pub fn sincronizar(conn: &Connection, pack: &Pack) -> Result<usize> {
    let pendientes: Vec<(i64, String)> = conn
        .prepare(
            "select c.id, c.body from chunks c
               left join chunk_vectors v on v.chunk_id = c.id
              where v.chunk_id is null",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    for (id, cuerpo) in &pendientes {
        let v = pack.embeber(cuerpo)?;
        tx.execute(
            "insert into chunk_vectors (chunk_id, vec) values (?1, ?2)",
            params![id, a_blob(&v)],
        )?;
    }
    tx.commit()?;
    Ok(pendientes.len())
}

/// (fragmentos con vector, fragmentos totales). Un hueco significa que hay
/// memorias que la capa vectorial no ve todavía.
pub fn cobertura(conn: &Connection) -> Result<(usize, usize)> {
    let con: i64 = conn.query_row("select count(*) from chunk_vectors", [], |r| r.get(0))?;
    let tot: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0))?;
    Ok((con as usize, tot as usize))
}

/// Las `limite` memorias más parecidas a `consulta`, con los mismos filtros
/// que aplica la búsqueda léxica. Cada memoria aparece una sola vez, con su
/// fragmento más parecido.
pub fn buscar(
    conn: &Connection,
    consulta: &[f32],
    q: &Query,
    limite: usize,
) -> Result<Vec<Acierto>> {
    let mut sql = String::from(
        "select m.id, v.chunk_id, v.vec
           from chunk_vectors v
           join chunks c on c.id = v.chunk_id
           join memories m on m.id = c.memory_id
          where m.status = 'active'",
    );
    let mut ps: Vec<Value> = Vec::new();
    if let Some(p) = &q.project {
        ps.push(Value::Text(p.clone()));
        sql.push_str(&format!(" and m.project = ?{}", ps.len()));
    }
    for t in &q.tags {
        ps.push(Value::Text(t.clone()));
        sql.push_str(&format!(
            " and exists (select 1 from memory_tags mt where mt.memory_id = m.id and mt.tag = ?{})",
            ps.len()
        ));
    }
    let mut stmt = conn.prepare(&sql)?;
    let filas = stmt.query_map(rusqlite::params_from_iter(ps.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))
    })?;

    let mut mejor: HashMap<i64, (i64, f32)> = HashMap::new();
    for fila in filas {
        let (mid, cid, blob) = fila?;
        let v = de_blob(&blob);
        if v.len() != consulta.len() {
            continue; // dimensiones distintas: vector de otro paquete
        }
        let cos: f32 = v.iter().zip(consulta).map(|(a, b)| a * b).sum();
        let e = mejor.entry(mid).or_insert((cid, f32::NEG_INFINITY));
        if cos > e.1 {
            *e = (cid, cos);
        }
    }
    let mut out: Vec<Acierto> = mejor
        .into_iter()
        .map(|(memory_id, (chunk_id, coseno))| Acierto { memory_id, chunk_id, coseno })
        .collect();
    // Mayor coseno primero; a igual coseno, menor id, por la misma razón de
    // determinismo que en `fusion::rrf` y en `search::run_explained`.
    out.sort_by(|a, b| {
        b.coseno
            .partial_cmp(&a.coseno)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.memory_id.cmp(&b.memory_id))
    });
    out.truncate(limite);
    Ok(out)
}
```

Agregar en `src/lib.rs`: `pub mod vecstore;`

- [ ] **Paso 4: correr los tests y verificar que pasan**

Correr: `cargo test --test vecstore_test`
Esperado: 8 tests PASS.

- [ ] **Paso 5: commit**

```bash
git add src/vecstore.rs src/lib.rs tests/vecstore_test.rs
git commit -m "feat: guardar, sincronizar y buscar vectores"
```

---

### Task 5: Configuración y fusión dentro de la búsqueda

**Archivos:**
- Crear: `src/config.rs`
- Crear: `tests/config_test.rs`
- Modificar: `src/search.rs`, `src/lib.rs`, `src/main.rs`, `src/mcp.rs`,
  `src/evalrun.rs`, `tests/search_test.rs`
- Modificar (llamadas): cualquier test que llame a `search::run` o
  `search::run_explained`

**Interfaces:**
- Consume: `fusion::rrf` (Task 2), `vecstore::buscar` (Task 4), `pack::Pack` (Task 1).
- Produce:
  - `config::Config { pub peso_bm25: f64 }` con `Default` = 0,25.
  - `config::cargar(dir: &Path) -> Result<Config>`
  - `search::Contexto<'a> { pub pack: Option<&'a Pack>, pub peso_bm25: f64 }`
    con `Contexto::solo_bm25() -> Contexto<'static>`
  - `search::run(&Connection, &Query, &Contexto) -> Result<Vec<SearchHit>>`
  - `search::run_explained(&Connection, &Query, &Contexto) -> Result<Vec<Explained>>`
  - `search::Explained { hit: SearchHit, bm25: Option<f64>, coseno: Option<f32> }`

Las firmas cambian a propósito. Un `run` que ignore la capa vectorial en
silencio sería exactamente el modo de falla que este diseño quiere evitar:
pasar el contexto obliga a cada llamador a decidir.

- [ ] **Paso 1: escribir el test de configuración que falla**

Crear `tests/config_test.rs`:

```rust
use anathem::config;

#[test]
fn sin_archivo_usa_el_peso_por_defecto() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(config::cargar(d.path()).unwrap().peso_bm25, 0.25);
}

#[test]
fn lee_el_peso_del_archivo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"peso_bm25": 1.0}"#).unwrap();
    assert_eq!(config::cargar(d.path()).unwrap().peso_bm25, 1.0);
}

#[test]
fn un_archivo_roto_falla_ruidosamente() {
    // Igual que tags.json: una configuración inválida no puede degradarse en
    // silencio a los valores por defecto, porque el usuario creería que su
    // ajuste está aplicado.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), "{esto no es json").unwrap();
    let e = config::cargar(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn un_peso_negativo_se_rechaza() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"peso_bm25": -1.0}"#).unwrap();
    assert!(config::cargar(d.path()).is_err());
}
```

- [ ] **Paso 2: correr y verificar que falla**

Correr: `cargo test --test config_test`
Esperado: no compila, `unresolved import anathem::config`.

- [ ] **Paso 3: escribir `src/config.rs`**

```rust
//! Ajustes que viven en `config.json`, junto a la base de datos.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Cuánto pesa el ranking léxico frente al vectorial al fusionar.
    /// Con 1,0 pesan igual; con 0,25 manda el vectorial.
    #[serde(default = "peso_por_defecto")]
    pub peso_bm25: f64,
}

fn peso_por_defecto() -> f64 {
    0.25
}

impl Default for Config {
    fn default() -> Self {
        Self { peso_bm25: peso_por_defecto() }
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
    Ok(c)
}
```

Agregar en `src/lib.rs`: `pub mod config;`

- [ ] **Paso 4: correr y verificar que pasa**

Correr: `cargo test --test config_test`
Esperado: 4 tests PASS.

- [ ] **Paso 5: escribir el test de fusión en la búsqueda que falla**

Agregar al final de `tests/search_test.rs`:

```rust
mod apoyo;

#[test]
fn sin_paquete_el_resultado_es_el_de_siempre() {
    let (_d, conn) = fixture();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("a"));
}

#[test]
fn con_paquete_aparece_lo_que_bm25_no_encuentra() {
    // El fixture mini solo conoce "hola", "mundo" y "s". Una memoria cuyo
    // cuerpo es "mundo" no matchea la consulta "hola" por bm25, pero sí se
    // parece por vector, así que la fusión tiene que sacarla a la superficie.
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::abrir(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    seed(&conn, "y", "p", "mundo");
    anathem::vecstore::sincronizar(&conn, &p).unwrap();

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let solo = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(solo.len(), 1, "bm25 solo encuentra la que comparte la palabra");

    let ctx = search::Contexto { pack: Some(&p), peso_bm25: 0.25 };
    let con = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(con.len(), 2, "la fusión debe traer también la vectorial");
    assert_eq!(con[0].ext_id.as_deref(), Some("x"), "la exacta sigue primera");
}

#[test]
fn las_memorias_fijadas_siguen_primero_con_fusion() {
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::abrir(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    let fijada = seed(&conn, "y", "p", "mundo");
    anathem::maint::pin(&conn, fijada, true).unwrap();
    anathem::vecstore::sincronizar(&conn, &p).unwrap();

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let ctx = search::Contexto { pack: Some(&p), peso_bm25: 0.25 };
    let hits = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("y"), "la fijada va primera igual");
}
```

Los tests existentes de `search_test.rs` hay que actualizarlos para pasar
`&search::Contexto::solo_bm25()` como tercer argumento. **Sus expectativas no
cambian**: si alguna deja de pasar, es un bug de esta tarea, no un test a
ajustar.

- [ ] **Paso 6: correr y verificar que falla**

Correr: `cargo test --test search_test`
Esperado: no compila, `run` toma 2 argumentos.

- [ ] **Paso 7: modificar `src/search.rs`**

Agregar arriba:

```rust
use crate::pack::Pack;
```

Reemplazar la definición de `Explained` por:

```rust
/// Resultado con las señales crudas expuestas, para `--explain` y para quien
/// necesite el número y no solo el orden. Un resultado que entró solo por
/// bm25 no tiene coseno, y uno que entró solo por vector no tiene bm25.
#[derive(Debug, Clone)]
pub struct Explained {
    pub hit: SearchHit,
    pub bm25: Option<f64>,
    pub coseno: Option<f32>,
}

/// Qué recuperadores están disponibles para esta búsqueda.
pub struct Contexto<'a> {
    pub pack: Option<&'a Pack>,
    pub peso_bm25: f64,
}

impl Contexto<'static> {
    /// Búsqueda sin capa vectorial: el comportamiento histórico.
    pub fn solo_bm25() -> Contexto<'static> {
        Contexto { pack: None, peso_bm25: 1.0 }
    }
}
```

Reemplazar `run_explained` y `run` por:

```rust
/// Trae un `SearchHit` armado alrededor de un fragmento concreto. Lo usa la
/// fusión para las memorias que encontró el vector y bm25 no: de esas no
/// tenemos fila del pool léxico, pero sí sabemos qué fragmento ganó.
fn hit_de_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<(SearchHit, i64)>> {
    let r = conn.query_row(
        "select m.id, m.ext_id, m.project, m.title, c.body, m.pinned
           from chunks c join memories m on m.id = c.memory_id
          where c.id = ?1",
        [chunk_id],
        |r| {
            Ok((
                SearchHit {
                    memory_id: r.get(0)?,
                    ext_id: r.get(1)?,
                    project: r.get(2)?,
                    title: r.get(3)?,
                    snippet: r.get(4)?,
                    score: 0.0,
                },
                r.get::<_, i64>(5)?,
            ))
        },
    );
    match r {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn run_explained(
    conn: &Connection,
    q: &Query,
    ctx: &Contexto,
) -> Result<Vec<Explained>> {
    let m = to_match(&q.text);

    // --- recuperación léxica: idéntica a como era ---
    let mut best: HashMap<i64, (SearchHit, i64)> = HashMap::new();
    let mut orden_bm25: Vec<i64> = Vec::new();
    if !m.is_empty() {
        let mut rows = candidate_rows(conn, &m, q, "", Some(CANDIDATES))?;
        rows.extend(candidate_rows(conn, &m, q, " and m.pinned = 1", None)?);
        for (id, ext_id, project, title, body, score, pinned) in rows {
            match best.entry(id) {
                Entry::Occupied(mut e) => {
                    if score < e.get().0.score {
                        e.get_mut().0.snippet = body;
                        e.get_mut().0.score = score;
                    }
                }
                Entry::Vacant(e) => {
                    e.insert((
                        SearchHit { memory_id: id, ext_id, project, title, snippet: body, score },
                        pinned,
                    ));
                }
            }
        }
        let mut por_score: Vec<(i64, f64)> =
            best.iter().map(|(id, (h, _))| (*id, h.score)).collect();
        // bm25() devuelve negativos: más negativo es mejor.
        por_score.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0))
        });
        orden_bm25 = por_score.into_iter().map(|(id, _)| id).collect();
    }

    // --- recuperación vectorial, si hay paquete ---
    let mut cosenos: HashMap<i64, f32> = HashMap::new();
    let mut orden_vec: Vec<i64> = Vec::new();
    if let Some(p) = ctx.pack {
        if !q.text.trim().is_empty() {
            let cv = p.embeber(&q.text)?;
            for a in crate::vecstore::buscar(conn, &cv, q, CANDIDATES)? {
                cosenos.insert(a.memory_id, a.coseno);
                orden_vec.push(a.memory_id);
                if !best.contains_key(&a.memory_id) {
                    if let Some(par) = hit_de_chunk(conn, a.chunk_id)? {
                        best.insert(a.memory_id, par);
                    }
                }
            }
        }
    }

    if best.is_empty() {
        return Ok(Vec::new());
    }

    // --- orden final ---
    let fusionado: Vec<i64> = if orden_vec.is_empty() {
        orden_bm25
    } else {
        crate::fusion::rrf(&orden_bm25, &orden_vec, ctx.peso_bm25)
    };

    // Las fijadas siguen primero, como siempre: la fusión ordena dentro de
    // cada grupo, no lo atraviesa.
    let mut fijadas: Vec<i64> = Vec::new();
    let mut resto: Vec<i64> = Vec::new();
    for id in fusionado {
        match best.get(&id) {
            Some((_, 1)) => fijadas.push(id),
            Some(_) => resto.push(id),
            None => {}
        }
    }
    fijadas.extend(resto);

    let mut out = Vec::new();
    let mut spent = 0usize;
    for id in fijadas {
        if out.len() >= q.limit {
            break;
        }
        let (hit, _) = match best.remove(&id) {
            Some(v) => v,
            None => continue,
        };
        let cost = est_tokens(&hit.title) + est_tokens(&hit.snippet);
        if !out.is_empty() && spent + cost > q.budget_tokens {
            break;
        }
        spent += cost;
        let bm25 = if hit.score < 0.0 { Some(hit.score) } else { None };
        let coseno = cosenos.get(&id).copied();
        out.push(Explained { hit, bm25, coseno });
    }
    Ok(out)
}

pub fn run(conn: &Connection, q: &Query, ctx: &Contexto) -> Result<Vec<SearchHit>> {
    Ok(run_explained(conn, q, ctx)?.into_iter().map(|e| e.hit).collect())
}
```

- [ ] **Paso 8: actualizar los llamadores**

En `src/main.rs`, `src/mcp.rs` y `src/evalrun.rs`, cada llamada a
`search::run` o `search::run_explained` pasa a recibir un `&Contexto`.
Construirlo una sola vez por proceso, nunca por consulta: abrir el paquete
cuesta entre 40 y 120 milisegundos, contra los ~2 de una búsqueda.

En `src/main.rs`, agregar el ayudante:

```rust
/// Abre el paquete registrado, si lo hay. Devuelve `None` —degradando a bm25—
/// cuando no hay ninguno registrado o el archivo desapareció. Buscar nunca
/// puede fallar por culpa de la capa vectorial.
fn paquete_activo(
    conn: &rusqlite::Connection,
    dir: &Path,
) -> Result<Option<anathem::pack::Pack>> {
    let Some((nombre, _sha, _dims)) = anathem::vecstore::pack_registrado(conn)? else {
        return Ok(None);
    };
    let ruta = dir.join("paquetes").join(format!("{nombre}.anathem-pack"));
    if !ruta.exists() {
        eprintln!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            ruta.display()
        );
        return Ok(None);
    }
    Ok(Some(anathem::pack::Pack::abrir(&ruta)?))
}
```

Donde hoy se resuelve la búsqueda en `Cmd::Search`, construir:

```rust
let dir = path.parent().unwrap_or(std::path::Path::new("."));
let cfg = anathem::config::cargar(dir)?;
let p = paquete_activo(&conn, dir)?;
let ctx = anathem::search::Contexto { pack: p.as_ref(), peso_bm25: cfg.peso_bm25 };
```

En `Cmd::Serve` (`src/mcp.rs`), lo mismo una vez al arrancar el servidor, antes
del bucle de mensajes, y pasar el `&Contexto` a cada búsqueda.

En `src/evalrun.rs`, el harness recibe el `&Contexto` de quien lo llama, para
poder medir con y sin capa vectorial usando el mismo evalset.

- [ ] **Paso 9: correr todo y verificar que pasa**

Correr: `cargo test`
Esperado: todo verde, incluidos los tests viejos de `search_test.rs` con sus
expectativas originales.

- [ ] **Paso 10: commit**

```bash
git add src/config.rs src/search.rs src/lib.rs src/main.rs src/mcp.rs src/evalrun.rs tests/
git commit -m "feat: fusionar bm25 con el ranking vectorial en la busqueda"
```

---

### Task 6: Comandos `mem model`

**Archivos:**
- Modificar: `src/main.rs`
- Crear: `tests/model_cli_test.rs`

**Interfaces:**
- Consume: `pack::Pack`, `pack::sha256`, `vecstore::*`, `paquete_activo` (Task 5).
- Produce: subcomandos `mem model install <archivo>`, `mem model status`,
  `mem model remove`.

- [ ] **Paso 1: escribir los tests que fallan**

Crear `tests/model_cli_test.rs`:

```rust
mod apoyo;

use std::process::Command;

fn mem(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn instalar_registra_el_paquete_y_lo_reporta() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("mini-es"), "status debe nombrar el paquete: {txt}");
    assert!(txt.contains("1/1"), "status debe reportar cobertura completa: {txt}");
}

#[test]
fn instalar_rechaza_un_sha256_que_no_coincide() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    std::fs::write(
        d.path().join("mini.anathem-pack.sha256"),
        "0000000000000000000000000000000000000000000000000000000000000000  mini.anathem-pack\n",
    )
    .unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "un sha que no coincide tiene que fallar");
    assert!(String::from_utf8_lossy(&o.stderr).contains("sha256"));
}

#[test]
fn instalar_rechaza_un_archivo_que_no_es_paquete() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("cualquiera.anathem-pack");
    std::fs::write(&r, b"no soy un paquete").unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success());
}

#[test]
fn status_sin_paquete_lo_dice_y_no_falla() {
    let d = tempfile::tempdir().unwrap();
    let o = mem(d.path(), &["model", "status"]);
    assert!(o.status.success());
    let txt = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(txt.contains("sin paquete"), "salió: {txt}");
}

#[test]
fn quitar_el_paquete_vuelve_a_bm25_sin_romper_la_busqueda() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);

    let o = mem(d.path(), &["model", "remove"]);
    assert!(o.status.success());

    let s = mem(d.path(), &["model", "status"]);
    assert!(String::from_utf8_lossy(&s.stdout).contains("sin paquete"));

    let b = mem(d.path(), &["search", "hola"]);
    assert!(b.status.success(), "la búsqueda tiene que seguir andando");
    assert!(String::from_utf8_lossy(&b.stdout).contains("hola"));
}

#[test]
fn si_falta_el_archivo_del_paquete_la_busqueda_degrada_a_bm25() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    std::fs::remove_file(d.path().join("paquetes").join("mini-es.anathem-pack")).unwrap();

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "buscar no puede fallar por un paquete ausente");
    assert!(String::from_utf8_lossy(&o.stderr).contains("aviso"));
}
```

- [ ] **Paso 2: correr y verificar que falla**

Correr: `cargo test --test model_cli_test`
Esperado: FAIL, `mem model` no existe.

- [ ] **Paso 3: agregar el subcomando en `src/main.rs`**

En `enum Cmd`:

```rust
    /// Administra el paquete de vocabulario de la capa vectorial
    Model {
        #[command(subcommand)]
        accion: ModelCmd,
    },
```

Y el enum nuevo:

```rust
#[derive(Subcommand)]
enum ModelCmd {
    /// Instala un paquete desde un archivo local y reindexa
    Install {
        /// Ruta del archivo .anathem-pack
        archivo: PathBuf,
    },
    /// Muestra el paquete activo y la cobertura de vectores
    Status,
    /// Quita el paquete y sus vectores; la búsqueda vuelve a ser solo bm25
    Remove,
}
```

En el `match` de `main`:

```rust
        Cmd::Model { accion } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            match accion {
                ModelCmd::Install { archivo } => {
                    // 1. El sha256 se verifica acá y solo acá.
                    let sha = anathem::pack::sha256(&archivo)?;
                    let firma = archivo.with_file_name(format!(
                        "{}.sha256",
                        archivo.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if firma.exists() {
                        let esperado = std::fs::read_to_string(&firma)?;
                        let esperado = esperado.split_whitespace().next().unwrap_or("").to_string();
                        if esperado != sha {
                            anyhow::bail!(
                                "el sha256 no coincide: el archivo da {sha} y {} dice {esperado}",
                                firma.display()
                            );
                        }
                    }
                    // 2. Que abra antes de copiar nada.
                    let p = anathem::pack::Pack::abrir(&archivo)?;
                    let nombre = p.nombre().to_string();
                    let dims = p.dims();
                    // 3. Copiar al directorio de la base.
                    let destino_dir = dir.join("paquetes");
                    std::fs::create_dir_all(&destino_dir)?;
                    let destino = destino_dir.join(format!("{nombre}.anathem-pack"));
                    std::fs::copy(&archivo, &destino)?;
                    // 4. Los vectores del paquete anterior no son comparables
                    //    con los de este: se van todos antes de rehacerlos.
                    anathem::vecstore::olvidar_pack(&conn)?;
                    anathem::vecstore::registrar_pack(&conn, &nombre, &sha, dims)?;
                    let p = anathem::pack::Pack::abrir(&destino)?;
                    let n = anathem::vecstore::sincronizar(&conn, &p)?;
                    println!("paquete {nombre} instalado ({dims} dimensiones), {n} fragmentos indexados");
                }
                ModelCmd::Status => match anathem::vecstore::pack_registrado(&conn)? {
                    None => println!("sin paquete: la búsqueda usa solo bm25"),
                    Some((nombre, sha, dims)) => {
                        let (con, tot) = anathem::vecstore::cobertura(&conn)?;
                        let ruta = dir.join("paquetes").join(format!("{nombre}.anathem-pack"));
                        println!("paquete {nombre} ({dims} dimensiones, sha {})", &sha[..16.min(sha.len())]);
                        println!("cobertura: {con}/{tot} fragmentos");
                        if !ruta.exists() {
                            println!("FALTA el archivo {}: la búsqueda está degradada a bm25", ruta.display());
                        } else if con < tot {
                            println!("hay {} fragmentos sin vector; corré `mem reindex`", tot - con);
                        }
                    }
                },
                ModelCmd::Remove => {
                    if let Some((nombre, _, _)) = anathem::vecstore::pack_registrado(&conn)? {
                        let ruta = dir.join("paquetes").join(format!("{nombre}.anathem-pack"));
                        let _ = std::fs::remove_file(&ruta);
                        anathem::vecstore::olvidar_pack(&conn)?;
                        println!("paquete {nombre} quitado; la búsqueda vuelve a ser solo bm25");
                    } else {
                        println!("no había ningún paquete instalado");
                    }
                }
            }
        }
```

- [ ] **Paso 4: hacer que `mem save` y `mem reindex` sincronicen**

En `Cmd::Save`, después de `guardar_explicito`, y en `Cmd::Reindex`, después
de `edit::reindex`:

```rust
            if let Some(p) = paquete_activo(&conn, &dir)? {
                let n = anathem::vecstore::sincronizar(&conn, &p)?;
                if n > 0 {
                    println!("vectores nuevos: {n}");
                }
            }
```

Lo mismo en `Cmd::Migrate` y en la tool `save_memory` de `src/mcp.rs`.

- [ ] **Paso 5: correr y verificar que pasa**

Correr: `cargo test --test model_cli_test`
Esperado: 6 tests PASS.

- [ ] **Paso 6: correr todo**

Correr: `cargo test`
Esperado: verde.

- [ ] **Paso 7: commit**

```bash
git add src/main.rs src/mcp.rs tests/model_cli_test.rs
git commit -m "feat: comandos mem model install, status y remove"
```

---

### Task 7: Nunca mezclar vectores de paquetes distintos

**Archivos:**
- Modificar: `src/main.rs` (la función `paquete_activo` de Task 5)
- Modificar: `tests/model_cli_test.rs`

**Interfaces:**
- Consume: todo lo anterior.
- Produce: `paquete_activo` reindexa cuando el paquete abierto no es el
  registrado, en vez de buscar con vectores incompatibles.

Este es el único modo de falla que el diseño declara inaceptable. Degradar a
bm25 está bien y es casi silencioso; un índice mezclado devuelve resultados
plausibles y equivocados, que para una herramienta de memoria es peor que no
devolver nada.

- [ ] **Paso 1: escribir los tests que fallan**

Agregar a `tests/model_cli_test.rs`:

```rust
/// Variante del fixture con otro nombre y otras dimensiones, para forzar la
/// incompatibilidad.
fn escribir_pack_ancho(ruta: &std::path::Path, nombre: &str) {
    // Mismo tokenizador, pero 8 dimensiones en vez de 4.
    let tok = apoyo::TOKENIZER_MINI.as_bytes();
    let dims = 8usize;
    let filas: [[u8; 8]; 4] = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [10, 0, 0, 0, 0, 0, 0, 0],
        [0, 10, 0, 0, 0, 0, 0, 0],
        [0, 0, 10, 0, 0, 0, 0, 0],
    ];
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(filas.len() as u32).to_le_bytes());
    b.extend_from_slice(&(nombre.len() as u32).to_le_bytes());
    b.extend_from_slice(nombre.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims { b.extend_from_slice(&1.0f32.to_le_bytes()); }
    for _ in 0..dims { b.extend_from_slice(&0.0f32.to_le_bytes()); }
    for f in filas { b.extend_from_slice(&f); }
    std::fs::write(ruta, b).unwrap();
}

#[test]
fn cambiar_de_paquete_rehace_el_indice_en_vez_de_mezclar() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    escribir_pack_ancho(&b, "ancho-es");
    mem(d.path(), &["save", "hola mundo"]);

    mem(d.path(), &["model", "install", a.to_str().unwrap()]);
    mem(d.path(), &["model", "install", b.to_str().unwrap()]);

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("ancho-es"), "debe quedar el último instalado: {txt}");
    assert!(txt.contains("1/1"), "el índice debe estar completo y rehecho: {txt}");

    // Y los vectores guardados tienen que ser de 8 dimensiones, no de 4.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "los vectores viejos de 4 dimensiones sobrevivieron");
}

#[test]
fn un_paquete_sustituido_a_mano_dispara_el_reindexado() {
    // El registro dice "mini-es" pero el archivo con ese nombre es en realidad
    // otro paquete. Buscar no puede usar los vectores viejos.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    escribir_pack_ancho(&instalado, "mini-es"); // mismo nombre, otras dimensiones

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "debió reindexar con el paquete que está de verdad");
}
```

Agregar `rusqlite` a `[dev-dependencies]` si no está ya.

- [ ] **Paso 2: correr y verificar que falla**

Correr: `cargo test --test model_cli_test`
Esperado: FAIL, los vectores siguen midiendo 16 bytes.

- [ ] **Paso 3: reforzar `paquete_activo` en `src/main.rs`**

```rust
/// Abre el paquete registrado, si lo hay.
///
/// Devuelve `None` —degradando a bm25— cuando no hay paquete registrado o el
/// archivo desapareció: buscar nunca puede fallar por culpa de la capa
/// vectorial.
///
/// Si el paquete que hay en disco no es el que registró el índice (otro
/// nombre, u otras dimensiones), **reindexa antes de devolverlo**. Mezclar
/// vectores de dos paquetes no da un error: da resultados plausibles y
/// equivocados, que es el peor modo de falla para una herramienta de memoria.
fn paquete_activo(
    conn: &rusqlite::Connection,
    dir: &Path,
) -> Result<Option<anathem::pack::Pack>> {
    let Some((nombre, sha, dims)) = anathem::vecstore::pack_registrado(conn)? else {
        return Ok(None);
    };
    let ruta = dir.join("paquetes").join(format!("{nombre}.anathem-pack"));
    if !ruta.exists() {
        eprintln!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            ruta.display()
        );
        return Ok(None);
    }
    let p = anathem::pack::Pack::abrir(&ruta)?;
    if p.nombre() != nombre || p.dims() != dims {
        eprintln!(
            "aviso: el paquete en disco ({}, {} dimensiones) no es el que indexó esta base \
             ({nombre}, {dims}); reindexando para no mezclar vectores.",
            p.nombre(),
            p.dims()
        );
        anathem::vecstore::olvidar_pack(conn)?;
        let nuevo_sha = anathem::pack::sha256(&ruta)?;
        anathem::vecstore::registrar_pack(conn, p.nombre(), &nuevo_sha, p.dims())?;
        anathem::vecstore::sincronizar(conn, &p)?;
    } else {
        let _ = sha; // el sha se verifica al instalar, no acá
    }
    Ok(Some(p))
}
```

- [ ] **Paso 4: correr y verificar que pasa**

Correr: `cargo test --test model_cli_test`
Esperado: 8 tests PASS.

- [ ] **Paso 5: correr todo**

Correr: `cargo test`
Esperado: verde.

- [ ] **Paso 6: documentar el comando en el README**

Agregar a `README.md`, en la sección de comandos, una línea por cada
subcomando de `mem model` y un enlace a `docs/paquetes-de-vocabulario.md`.

- [ ] **Paso 7: commit**

```bash
git add src/main.rs tests/model_cli_test.rs README.md
git commit -m "fix: reindexar al cambiar de paquete en vez de mezclar vectores"
```

---

## Autorrevisión

**Cobertura de la especificación.** Cada sección tiene tarea: `src/vector.rs`
→ Tasks 1 y 4 (partido en `pack.rs` y `vecstore.rs` porque leer el formato y
persistir vectores son responsabilidades distintas; la especificación se
actualizó para reflejarlo); `src/fusion.rs` → Task 2; esquema → Task 3;
fuerza bruta → Task 4; flujo de búsqueda → Task 5; comandos → Task 6; tabla de
errores → Tasks 1 (archivo corrupto, versión), 6 (sha256, ausencia) y 7
(incompatibilidad); pruebas exigidas → todas cubiertas, incluida "la batería
actual pasa sin paquete" en el Paso 9 de Task 5.

**Fuera de alcance, y sigue estándolo:** descarga por HTTP, generación de
paquetes desde el binario, reranking, vectores en int8.

**Queda abierto, para el usuario y no para el implementador:** dónde se
hospedan los paquetes. Sin eso, `mem model install` solo le sirve a quien
genere el paquete en su propia máquina.
