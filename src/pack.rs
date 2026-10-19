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

#[derive(Debug)]
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

/// Recorta `n` bytes a partir de `*o`, avanza `*o` y devuelve el recorte.
///
/// Usa `checked_add` porque `n` casi siempre viene de un campo de largo leído
/// crudo del archivo: en un paquete corrupto puede ser cualquier valor de
/// hasta 64 bits, y sumarlo directo a `*o` desbordaría la aritmética de
/// punteros (con overflow-checks, el default de `cargo test`, eso paniquea
/// en vez de devolver un error). El desborde y el archivo simplemente
/// truncado terminan en el mismo `Err`, nombrando el archivo y el campo.
fn tomar<'a>(m: &'a [u8], o: &mut usize, n: usize, ruta: &Path, campo: &str) -> Result<&'a [u8]> {
    let fin = o
        .checked_add(n)
        .filter(|&fin| fin <= m.len())
        .with_context(|| {
            format!("el paquete {} termina antes de lo declarado: falta {campo}", ruta.display())
        })?;
    let s = &m[*o..fin];
    *o = fin;
    Ok(s)
}

fn u32_en(m: &[u8], o: &mut usize, ruta: &Path, campo: &str) -> Result<u32> {
    let f = tomar(m, o, 4, ruta, campo)?;
    Ok(u32::from_le_bytes(f.try_into().unwrap()))
}

fn u64_en(m: &[u8], o: &mut usize, ruta: &Path, campo: &str) -> Result<u64> {
    let f = tomar(m, o, 8, ruta, campo)?;
    Ok(u64::from_le_bytes(f.try_into().unwrap()))
}

fn f32s_en(m: &[u8], o: &mut usize, n: usize, ruta: &Path, campo: &str) -> Result<Vec<f32>> {
    let largo = n.checked_mul(4).with_context(|| {
        format!("el paquete {} declara un {campo} demasiado grande", ruta.display())
    })?;
    let f = tomar(m, o, largo, ruta, campo)?;
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
        let mut o = 4usize;
        let version = u32_en(&mapa, &mut o, ruta, "versión")?;
        if version != VERSION {
            bail!(
                "el paquete {} declara la versión {version} y este binario entiende la {VERSION}",
                ruta.display()
            );
        }
        let dims = u32_en(&mapa, &mut o, ruta, "dimensiones")? as usize;
        let n_tokens = u32_en(&mapa, &mut o, ruta, "cantidad de tokens")? as usize;
        let largo_nombre = u32_en(&mapa, &mut o, ruta, "largo del nombre")? as usize;
        if dims == 0 || n_tokens == 0 {
            bail!("el paquete {} declara 0 dimensiones o 0 tokens", ruta.display());
        }
        let nombre_bytes = tomar(&mapa, &mut o, largo_nombre, ruta, "nombre")?;
        let nombre = std::str::from_utf8(nombre_bytes)
            .with_context(|| format!("el nombre del paquete {} no es UTF-8 válido", ruta.display()))?
            .to_string();
        let largo_tok = u64_en(&mapa, &mut o, ruta, "largo del tokenizador")? as usize;
        let crudo = tomar(&mapa, &mut o, largo_tok, ruta, "tokenizador")?;
        let tok = Tokenizer::from_bytes(crudo).map_err(|e| {
            anyhow::anyhow!("el tokenizador del paquete {} no se pudo leer: {e}", ruta.display())
        })?;
        // Un paquete cuya tabla es más chica que el vocabulario del
        // tokenizador abre, instala y reporta cobertura completa: `embeber`
        // saltea las filas que no existen, y si faltan todas produce solo
        // vectores nulos sin ninguna señal de que algo anda mal. El
        // `continue` de `embeber` es la red correcta contra el pánico y se
        // queda donde está; la incoherencia se detecta acá, una sola vez,
        // cuando todavía se puede decir cuál es el archivo culpable.
        //
        // La cantidad que hay que comparar es la del vocabulario **del
        // modelo**, que es exactamente lo que el generador vuelca en la
        // tabla: `construir_paquete.py` escribe una fila por pieza de
        // `model.vocab` y deja `added_tokens` intacto. Con `true` la cuenta
        // sería la unión con los `added_tokens`, que pueden traer ids fuera
        // del rango del vocabulario podado, y entonces esta comprobación
        // rechazaría paquetes perfectamente sanos. Un `added_token` sin fila
        // lo cubre el `continue` de `embeber`, que es donde corresponde: un
        // token especial sin fila es normal; una tabla incompleta no.
        let piezas = tok.get_vocab_size(false);
        if n_tokens < piezas {
            bail!(
                "el paquete {} declara {n_tokens} filas pero su tokenizador tiene {piezas} \
                 piezas: la tabla está incompleta y el paquete produciría vectores nulos",
                ruta.display()
            );
        }
        let escalas = f32s_en(&mapa, &mut o, dims, ruta, "escalas")?;
        let minimos = f32s_en(&mapa, &mut o, dims, ruta, "mínimos")?;
        let tam_tabla = n_tokens.checked_mul(dims).with_context(|| {
            format!("el paquete {} declara una tabla demasiado grande", ruta.display())
        })?;
        let inicio_tabla = o;
        let fin_tabla = o.checked_add(tam_tabla).with_context(|| {
            format!("el paquete {} declara una tabla demasiado grande", ruta.display())
        })?;
        if mapa.len() < fin_tabla {
            bail!("la tabla del paquete {} está truncada", ruta.display());
        }
        Ok(Pack { mapa, tok, nombre, dims, n_tokens, escalas, minimos, inicio_tabla })
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
