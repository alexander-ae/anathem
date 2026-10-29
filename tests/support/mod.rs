// Every test binary includes this whole module but uses only some of the
// helpers, so the rest counts as "unused" from its point of view.
#![allow(dead_code)]

use std::path::Path;

/// A minimal Unigram tokenizer. Four pieces: the unknown one, two words and
/// a suffix. The suffix is there to check re-segmentation: "mundos" splits
/// into "▁mundo" + "s" instead of falling back to unknown.
pub const TOKENIZER_MINI: &str = r#"{
 "version":"1.0","truncation":null,"padding":null,"added_tokens":[],
 "normalizer":null,
 "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
 "post_processor":null,"decoder":null,
 "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
   "vocab":[["<unk>",0.0],["▁hola",-1.0],["▁mundo",-2.0],["s",-8.0]]}
}"#;

/// `TOKENIZER_MINI` plus an `added_token` whose id (7) falls **outside** the
/// model's vocabulary, which has four pieces (ids 0 to 3).
///
/// It is the shape of any pruned sentencepiece tokenizer: the generator dumps
/// one row per piece of `model.vocab` and leaves `added_tokens` as it found
/// them, with ids the trimming does not reassign.
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

/// Writes a pack with whatever table it is handed, using `TOKENIZER_MINI`.
///
/// The scale is 1.0 and the minimum 0.0 in every dimension, so the
/// dequantized value is the byte itself and the expected vectors are worked
/// out by hand. `n_tokens` comes from the length of `rows`, which is exactly
/// the field that has to be put out of step to test `Pack::open_pack`'s
/// validation.
pub fn write_pack_rows(pack_path: &Path, name: &str, dims: usize, rows: &[Vec<u8>]) {
    write_pack_with_tokenizer(pack_path, name, dims, rows, TOKENIZER_MINI);
}

/// `write_pack_rows` choosing the embedded tokenizer as well, so the
/// relation between table and vocabulary can be put out of step.
pub fn write_pack_with_tokenizer(
    pack_path: &Path,
    name: &str,
    dims: usize,
    rows: &[Vec<u8>],
    tokenizador: &str,
) {
    let tok = tokenizador.as_bytes();
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    b.extend_from_slice(&(name.len() as u32).to_le_bytes());
    b.extend_from_slice(name.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims {
        b.extend_from_slice(&1.0f32.to_le_bytes());
    }
    for _ in 0..dims {
        b.extend_from_slice(&0.0f32.to_le_bytes());
    }
    for f in rows {
        assert_eq!(f.len(), dims, "cada fila tiene que medir dims bytes");
        b.extend_from_slice(f);
    }
    std::fs::write(pack_path, b).unwrap();
}

/// Writes a pack of 4 tokens and 4 dimensions at `path`.
///
/// Rows: `<unk>`=[0,0,0,0], `▁hola`=[10,0,0,0], `▁mundo`=[0,10,0,0],
/// `s`=[0,0,10,0].
pub fn write_pack_mini(pack_path: &Path, name: &str) {
    write_pack_rows(
        pack_path,
        name,
        4,
        &[vec![0, 0, 0, 0], vec![10, 0, 0, 0], vec![0, 10, 0, 0], vec![0, 0, 10, 0]],
    );
}

/// A pack of the **same** 4 dimensions as `write_pack_mini` but with the
/// table permuted: `▁hola` lands on axis 3 and `▁mundo` on 2.
///
/// It is the dangerous case of mixing, and the one the format makes real:
/// since it fixes 256 dimensions for every pack, the blobs of two different
/// packs are exactly the same size and nothing tells them apart short of
/// looking at where they point.
pub fn write_pack_permuted(pack_path: &Path, name: &str) {
    write_pack_rows(
        pack_path,
        name,
        4,
        &[vec![0, 0, 0, 0], vec![0, 0, 0, 10], vec![0, 0, 10, 0], vec![0, 10, 0, 0]],
    );
}

/// A pack whose table is all zeros: it opens, installs and produces nothing
/// but null vectors.
pub fn write_pack_null(pack_path: &Path, name: &str) {
    write_pack_rows(pack_path, name, 4, &vec![vec![0u8; 4]; 4]);
}

/// Reads a vector from `chunk_vectors` as a `Vec<f32>`.
pub fn read_vector(conn: &rusqlite::Connection, sql: &str) -> Vec<f32> {
    let blob: Vec<u8> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
    blob.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

/// Rewrites a single byte of the file, so as to build corrupt packs.
pub fn corrupt_byte(pack_path: &Path, offset: usize, value: u8) {
    let mut b = std::fs::read(pack_path).unwrap();
    b[offset] = value;
    std::fs::write(pack_path, b).unwrap();
}

/// Registers `p` as the database's pack and syncs under that identity.
///
/// `vecstore::sync` demands the registered identity so it can check, inside
/// each batch's transaction, that it is still the current one. Registering
/// first is not test ceremony: in real life vectors only exist once a pack is
/// installed, and `install` leaves the registry set before syncing.
pub fn register_and_sync(
    conn: &rusqlite::Connection,
    p: &anathem::pack::Pack,
) -> anathem::vecstore::SyncOutcome {
    let id = (p.name().to_string(), "sha-de-prueba".to_string(), p.dims());
    anathem::vecstore::register_pack(conn, &id.0, &id.1, id.2).unwrap();
    anathem::vecstore::sync(conn, p, &id).unwrap()
}
