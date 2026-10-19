// Cada binario de test incluye este módulo entero pero usa solo algunos
// ayudantes, así que el resto queda "sin usar" desde su punto de vista.
#![allow(dead_code)]

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

/// `TOKENIZER_MINI` más un `added_token` cuyo id (7) cae **fuera** del
/// vocabulario del modelo, que tiene cuatro piezas (ids 0 a 3).
///
/// Es la forma de cualquier tokenizador sentencepiece podado: el generador
/// vuelca una fila por pieza de `model.vocab` y deja `added_tokens` como
/// venía, con ids que el recorte no reasigna.
pub const TOKENIZER_CON_AGREGADO_FUERA_DE_RANGO: &str = r#"{
 "version":"1.0","truncation":null,"padding":null,
 "added_tokens":[{"id":7,"content":"<mask>","single_word":false,"lstrip":false,
                  "rstrip":false,"normalized":false,"special":true}],
 "normalizer":null,
 "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
 "post_processor":null,"decoder":null,
 "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
   "vocab":[["<unk>",0.0],["▁hola",-1.0],["▁mundo",-2.0],["s",-8.0]]}
}"#;

/// Escribe un paquete con la tabla que se le pase, usando `TOKENIZER_MINI`.
///
/// La escala es 1,0 y el mínimo 0,0 en cada dimensión, así el valor
/// descuantizado es el byte tal cual y los vectores esperados se calculan a
/// mano. `n_tokens` sale del largo de `filas`, que es justo el campo que hay
/// que poder descuadrar para probar la validación de `Pack::abrir`.
pub fn escribir_pack_filas(ruta: &Path, nombre: &str, dims: usize, filas: &[Vec<u8>]) {
    escribir_pack_con_tokenizador(ruta, nombre, dims, filas, TOKENIZER_MINI);
}

/// `escribir_pack_filas` eligiendo también el tokenizador embebido, para
/// poder descuadrar la relación entre la tabla y el vocabulario.
pub fn escribir_pack_con_tokenizador(
    ruta: &Path,
    nombre: &str,
    dims: usize,
    filas: &[Vec<u8>],
    tokenizador: &str,
) {
    let tok = tokenizador.as_bytes();
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
        assert_eq!(f.len(), dims, "cada fila tiene que medir dims bytes");
        b.extend_from_slice(f);
    }
    std::fs::write(ruta, b).unwrap();
}

/// Escribe un paquete de 4 tokens y 4 dimensiones en `ruta`.
///
/// Filas: `<unk>`=[0,0,0,0], `▁hola`=[10,0,0,0], `▁mundo`=[0,10,0,0],
/// `s`=[0,0,10,0].
pub fn escribir_pack_mini(ruta: &Path, nombre: &str) {
    escribir_pack_filas(
        ruta,
        nombre,
        4,
        &[vec![0, 0, 0, 0], vec![10, 0, 0, 0], vec![0, 10, 0, 0], vec![0, 0, 10, 0]],
    );
}

/// Un paquete de las **mismas** 4 dimensiones que `escribir_pack_mini` pero
/// con la tabla permutada: `▁hola` cae en el eje 3 y `▁mundo` en el 2.
///
/// Es el caso peligroso de la mezcla, y el que el formato hace real: como
/// fija 256 dimensiones para todo paquete, los blobs de dos paquetes
/// distintos miden exactamente lo mismo y nada los distingue salvo mirar
/// hacia dónde apuntan.
pub fn escribir_pack_permutado(ruta: &Path, nombre: &str) {
    escribir_pack_filas(
        ruta,
        nombre,
        4,
        &[vec![0, 0, 0, 0], vec![0, 0, 0, 10], vec![0, 0, 10, 0], vec![0, 10, 0, 0]],
    );
}

/// Un paquete cuya tabla es todo ceros: abre, instala y produce solo
/// vectores nulos.
pub fn escribir_pack_nulo(ruta: &Path, nombre: &str) {
    escribir_pack_filas(ruta, nombre, 4, &vec![vec![0u8; 4]; 4]);
}

/// Lee un vector de `chunk_vectors` como `Vec<f32>`.
pub fn leer_vector(conn: &rusqlite::Connection, sql: &str) -> Vec<f32> {
    let blob: Vec<u8> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
    blob.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

/// Reescribe un byte suelto del archivo, para construir paquetes corruptos.
pub fn corromper(ruta: &Path, offset: usize, valor: u8) {
    let mut b = std::fs::read(ruta).unwrap();
    b[offset] = valor;
    std::fs::write(ruta, b).unwrap();
}

/// Registra `p` como el paquete de la base y sincroniza con esa identidad.
///
/// `vecstore::sincronizar` exige la identidad registrada para poder
/// comprobar, dentro de la transacción de cada lote, que sigue siendo la
/// vigente. Registrar antes no es ceremonia de test: en la vida real los
/// vectores solo existen cuando hay un paquete instalado, y `instalar` deja
/// el registro puesto antes de sincronizar.
pub fn registrar_y_sincronizar(
    conn: &rusqlite::Connection,
    p: &anathem::pack::Pack,
) -> anathem::vecstore::Sincronizacion {
    let id = (p.nombre().to_string(), "sha-de-prueba".to_string(), p.dims());
    anathem::vecstore::registrar_pack(conn, &id.0, &id.1, id.2).unwrap();
    anathem::vecstore::sincronizar(conn, p, &id).unwrap()
}
