mod support;

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
fn install_registers_the_pack_and_reports_it() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("mini-es"), "status debe nombrar el paquete: {txt}");
    assert!(txt.contains("1/1"), "status debe reportar cobertura completa: {txt}");
}

#[test]
fn install_rejects_a_sha256_that_does_not_match() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
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
fn install_rejects_a_file_that_is_not_a_pack() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("cualquiera.anathem-pack");
    std::fs::write(&r, b"no soy un paquete").unwrap();
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success());
}

#[test]
fn status_with_no_pack_says_so_and_does_not_fail() {
    let d = tempfile::tempdir().unwrap();
    let o = mem(d.path(), &["model", "status"]);
    assert!(o.status.success());
    let txt = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(txt.contains("no pack"), "salió: {txt}");
}

#[test]
fn removing_the_pack_falls_back_to_bm25_without_breaking_search() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);

    let o = mem(d.path(), &["model", "remove"]);
    assert!(o.status.success());

    let s = mem(d.path(), &["model", "status"]);
    assert!(String::from_utf8_lossy(&s.stdout).contains("no pack"));

    let b = mem(d.path(), &["search", "hola"]);
    assert!(b.status.success(), "la búsqueda tiene que seguir andando");
    assert!(String::from_utf8_lossy(&b.stdout).contains("hola"));
}

#[test]
fn a_missing_pack_file_degrades_search_to_bm25() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    std::fs::remove_file(d.path().join("paquetes").join("mini-es.anathem-pack")).unwrap();

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "buscar no puede fallar por un paquete ausente");
    assert!(String::from_utf8_lossy(&o.stderr).contains("warning"));
}

/// A variant of the fixture with a different name and different dimensions,
/// to force the mismatch.
fn write_pack_wide(pack_path: &std::path::Path, name: &str) {
    // Same tokenizer, but 8 dimensions instead of 4.
    let tok = support::TOKENIZER_MINI.as_bytes();
    let dims = 8usize;
    let rows: [[u8; 8]; 4] = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [10, 0, 0, 0, 0, 0, 0, 0],
        [0, 10, 0, 0, 0, 0, 0, 0],
        [0, 0, 10, 0, 0, 0, 0, 0],
    ];
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"ANPK");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(dims as u32).to_le_bytes());
    b.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    b.extend_from_slice(&(name.len() as u32).to_le_bytes());
    b.extend_from_slice(name.as_bytes());
    b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
    b.extend_from_slice(tok);
    for _ in 0..dims { b.extend_from_slice(&1.0f32.to_le_bytes()); }
    for _ in 0..dims { b.extend_from_slice(&0.0f32.to_le_bytes()); }
    for f in rows { b.extend_from_slice(&f); }
    // An installed pack is 0400 (finding B-2), and some of these tests
    // overwrite one in place on purpose to simulate a swap by hand. Taking
    // the write bit back is the same deliberate step that would take
    // outside the tests.
    #[cfg(unix)]
    if pack_path.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(pack_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(pack_path, b).unwrap();
    support::write_sidecar(pack_path);
}

#[test]
fn switching_packs_rebuilds_the_index_instead_of_mixing() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    write_pack_wide(&b, "ancho-es");
    mem(d.path(), &["save", "hola mundo"]);

    mem(d.path(), &["model", "install", a.to_str().unwrap()]);
    mem(d.path(), &["model", "install", b.to_str().unwrap()]);

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("ancho-es"), "debe quedar el último instalado: {txt}");
    assert!(txt.contains("1/1"), "el índice debe estar completo y rehecho: {txt}");

    // And the stored vectors have to be 8-dimensional, not 4.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "los vectores viejos de 4 dimensiones sobrevivieron");
}

#[test]
fn a_pack_swapped_by_hand_triggers_the_reindex() {
    // The registry says "mini-es" but the file under that name is in fact a
    // different pack. Search cannot use the old vectors.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    write_pack_wide(&instalado, "mini-es"); // mismo name, otras dimensiones

    let o = mem(d.path(), &["search", "hola"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let largo: i64 = conn
        .query_row("select length(vec) from chunk_vectors limit 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(largo, 8 * 4, "debió reindexar con el paquete que está de verdad");
}

#[test]
fn a_name_mismatch_leaves_the_registry_consistent_with_disk() {
    // The registry says "mini-es", and the file at
    // packs/mini-es.anathem-pack has the SAME dimensions (4) but is in fact
    // "otro-es": same vector length, different vocabulary. Mixing would be
    // numerically valid and semantically garbage — the most dangerous case,
    // because nothing in the blob's length gives it away.
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", a.to_str().unwrap()]);

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    support::write_pack_mini(&instalado, "otro-es"); // mismas dims, otro name

    let o1 = mem(d.path(), &["search", "hola"]);
    assert!(o1.status.success(), "{}", String::from_utf8_lossy(&o1.stderr));

    // The registry has to end up pointing at a file that really exists: the
    // registered name and the name of the file on disk have to match.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let (name, _sha, _dims) = anathem::vecstore::registered_pack(&conn).unwrap().unwrap();
    assert_eq!(name, "otro-es", "el registro debe reflejar el nombre real del paquete");
    let registered_path = d.path().join("paquetes").join(format!("{name}.anathem-pack"));
    assert!(
        registered_path.exists(),
        "el archivo tiene que vivir donde dice el registro: {}",
        registered_path.display()
    );

    // A second search has to find the pack without warning again that the
    // file is missing, and without degrading to bm25.
    let o2 = mem(d.path(), &["search", "hola"]);
    assert!(o2.status.success(), "{}", String::from_utf8_lossy(&o2.stderr));
    let err2 = String::from_utf8_lossy(&o2.stderr).to_string();
    assert!(
        !err2.contains("missing"),
        "la segunda búsqueda no puede decir que falta el archivo: {err2}"
    );
}

#[test]
fn reinstalling_from_the_installed_path_does_not_destroy_the_pack() {
    // `mem model install packs/mini-es.anathem-pack` — reinstalling "to
    // force the reindex", or a script that always installs from the
    // canonical path — copied the file over itself and truncated it to zero
    // bytes. And since the `open_pack` that would have caught it ran AFTER
    // the `forget_pack`, the result was the pack destroyed on disk, zero
    // vectors, and the registry pointing at garbage under the old sha, which
    // gives nothing away. Unrecoverable without an external copy.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    let tam = std::fs::metadata(&instalado).unwrap().len();
    assert!(tam > 0);

    let o = mem(d.path(), &["model", "install", instalado.to_str().unwrap()]);
    assert!(
        o.status.success(),
        "reinstalar desde la ruta ya instalada tiene que funcionar: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        std::fs::metadata(&instalado).unwrap().len(),
        tam,
        "el paquete quedó truncado en disco"
    );

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(txt.contains("1/1"), "los vectores tienen que seguir estando: {txt}");

    let b = mem(d.path(), &["search", "hola"]);
    assert!(b.status.success());
    let err = String::from_utf8_lossy(&b.stderr).to_string();
    assert!(!err.contains("warning"), "no puede quedar avisando para siempre: {err}");

    // And the install leaves no temporaries lying around in `packs/` — the
    // pack and the sidecar it writes beside it are the whole contents.
    let extras: Vec<String> = std::fs::read_dir(d.path().join("paquetes"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack" && n != "mini-es.anathem-pack.sha256")
        .collect();
    assert!(extras.is_empty(), "quedaron archivos a medio instalar: {extras:?}");
}

#[test]
fn a_pack_that_recognizes_nothing_is_rejected_without_touching_the_current_one() {
    // A pack whose table does not cover the vocabulary this memory uses
    // installs cleanly, reports full coverage and produces nothing but null
    // vectors: vector search stops contributing anything and nothing says
    // so. On top of that, installing it used to overwrite the pack that
    // worked before even checking that the new one was any good.
    let d = tempfile::tempdir().unwrap();
    let bueno = d.path().join("bueno.anathem-pack");
    support::write_pack_mini(&bueno, "mini-es");
    let nulo = d.path().join("nulo.anathem-pack");
    support::write_pack_null(&nulo, "nulo-es");

    mem(d.path(), &["save", "hola mundo"]);
    let o = mem(d.path(), &["model", "install", bueno.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let o = mem(d.path(), &["model", "install", nulo.to_str().unwrap()]);
    assert!(
        !o.status.success(),
        "un paquete que solo produce vectores nulos no puede instalarse en silencio"
    );

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(
        txt.contains("mini-es"),
        "el paquete que estaba tiene que sobrevivir a una instalación rechazada: {txt}"
    );
    assert!(txt.contains("1/1"), "y sus vectores también: {txt}");
}

#[test]
fn status_does_not_count_vectors_search_discards_as_covered() {
    // `coverage` did a `count(*)`, so it reported 1/1 while
    // `search_vectors` discarded that very row for an incompatible
    // dimension. The one diagnostic the spec designates for making the gap
    // visible reported perfect health precisely when the gap was total.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    mem(d.path(), &["model", "install", r.to_str().unwrap()]);

    // An 8-dimensional vector where the active pack has 4: what any of the
    // mixtures this branch is after leaves behind.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    conn.execute("update chunk_vectors set vec = zeroblob(32)", []).unwrap();
    drop(conn);

    let s = mem(d.path(), &["model", "status"]);
    let txt = String::from_utf8_lossy(&s.stdout).to_string();
    assert!(
        txt.contains("0/1"),
        "un vector que la búsqueda descarta por incompatible no es cobertura: {txt}"
    );
}

/// How many `pack_instalando` rows there are in `meta`. Queried with raw SQL
/// on purpose: that way this test does not depend on an API for reading the
/// mark existing, and fails for what it asserts rather than for not
/// compiling.
fn install_marks(db: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row("select count(*) from meta where key = 'pack_instalando'", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn an_install_cut_short_after_the_rename_does_not_keep_querying_the_old_build() {
    // Between the `rename` — which has already replaced the file in
    // `packs/` — and the `adopt_pack` that was going to describe it, there
    // is a window. A SIGKILL there leaves the NEW build on disk with `meta`
    // describing the OLD one.
    //
    // It is the case no guard sees: if the new build has the same name and
    // the same size as the old — updating `en-es`, the commonest reason to
    // reinstall — name and dimensions match, and the registered sha is
    // discarded on purpose so as not to hash 22 MB on every start. Result:
    // queries embedded with the new pack against the old one's vectors,
    // indefinitely, and `model status` reporting full coverage.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let old = d.path().join("viejo.anathem-pack");
    let new_id = d.path().join("nuevo.anathem-pack");
    support::write_pack_mini(&old, "mini-es"); // ▁hola -> eje 0
    support::write_pack_permuted(&new_id, "mini-es"); // mismo name y dims, ▁hola -> eje 3

    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", old.to_str().unwrap()]).status.success());
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let v = support::read_vector(&conn, "select vec from chunk_vectors");
        assert!(v[0] > 0.5, "precondición: el índice es del build viejo, salió {v:?}");
        // Killing the process exactly between the rename and the
        // `adopt_pack` is not something a test can ask for. Instead the
        // `adopt_pack` is made to fail, which leaves precisely the same
        // state: the file already replaced and `meta` still describing the
        // old one.
        conn.execute_batch(
            "create trigger cortar_la_instalacion before delete on chunk_vectors
             begin select raise(abort, 'instalación interrumpida'); end;",
        )
        .unwrap();
    }

    let o = mem(d.path(), &["model", "install", new_id.to_str().unwrap()]);
    assert!(!o.status.success(), "esta instalación tenía que quedar cortada");
    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    assert_eq!(
        std::fs::read(&instalado).unwrap(),
        std::fs::read(&new_id).unwrap(),
        "precondición: el rename ya pasó y en disco está el build nuevo"
    );

    {
        // A killed process leaves no triggers behind; this one was the instrument.
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("drop trigger cortar_la_instalacion").unwrap();
    }

    let o = mem(d.path(), &["search", "hola"]);
    assert!(
        o.status.success(),
        "buscar no puede fallar por esto: {}",
        String::from_utf8_lossy(&o.stderr)
    );

    let conn = rusqlite::Connection::open(&db).unwrap();
    let v = support::read_vector(&conn, "select vec from chunk_vectors");
    assert!(
        v[3] > 0.5,
        "se está consultando con el paquete nuevo contra vectores del viejo: {v:?}"
    );
    drop(conn);
    assert_eq!(
        install_marks(&db),
        0,
        "resuelta la incoherencia, la marca no puede quedar reindexando para siempre"
    );
}

#[test]
fn a_complete_install_leaves_no_mark_behind() {
    // The other half: were the mark to survive a successful install, every
    // later search would reindex the whole corpus, forever.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", r.to_str().unwrap()]).status.success());

    assert_eq!(install_marks(&db), 0, "la instalación terminó: la marca sobra");
    let o = mem(d.path(), &["search", "hola"]);
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.is_empty(), "una instalación sana no puede dejar avisando a cada búsqueda: {err}");
}

#[test]
fn an_install_failing_before_touching_the_file_leaves_no_mark() {
    // If the rename never even happened, the registered pack's file is
    // intact and the mark describes no risk at all: leaving it set would
    // cost a reindex of the whole corpus for nothing.
    //
    // The rename is made to fail with the target occupied by a non-empty
    // directory, which gives EISDIR for any uid and does not depend on
    // permissions.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);

    let packs = d.path().join("paquetes");
    std::fs::create_dir_all(&packs).unwrap();
    let target = packs.join("mini-es.anathem-pack");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("ocupado"), b"x").unwrap();

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "el rename imposible tiene que hacer fallar la instalación");
    assert_eq!(install_marks(&db), 0, "el archivo no se tocó: la marca no corresponde");

    // And no temporaries are left lying around either.
    let extras: Vec<String> = std::fs::read_dir(&packs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "mini-es.anathem-pack")
        .collect();
    assert!(extras.is_empty(), "quedaron archivos a medio instalar: {extras:?}");
}

/// The sidecar is what `tools/build_pack.py` writes beside every pack it
/// builds, and what the release carries beside the one it publishes. A pack
/// arriving without it is a pack nothing can check, and until now that was
/// waved through without a word — the installer script refuses in the same
/// situation, so the two halves of the same decision disagreed.
#[test]
fn install_refuses_a_pack_with_no_sha256_beside_it() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    std::fs::remove_file(d.path().join("mini.anathem-pack.sha256")).unwrap();
    mem(d.path(), &["save", "hola mundo"]);

    let o = mem(d.path(), &["model", "install", r.to_str().unwrap()]);
    assert!(!o.status.success(), "sin sha256 al lado no se puede verificar nada");
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.contains("sha256"), "tiene que decir qué falta: {err}");
    assert!(err.contains("--no-verify"), "y cómo seguir igual a propósito: {err}");
}

#[test]
fn no_verify_installs_a_pack_that_has_no_sha256() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    std::fs::remove_file(d.path().join("mini.anathem-pack.sha256")).unwrap();
    mem(d.path(), &["save", "hola mundo"]);

    let o = mem(d.path(), &["model", "install", "--no-verify", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let s = mem(d.path(), &["model", "status"]);
    assert!(String::from_utf8_lossy(&s.stdout).contains("mini-es"));
}

/// Reinstalling from `packs/` is a real flow — it is how an index gets
/// rebuilt — and it would break the moment the sidecar became mandatory,
/// because the install copied the pack and left its sidecar behind. So the
/// install writes one next to the pack it puts in place.
#[test]
fn the_installed_pack_carries_its_sha256_so_reinstalling_still_verifies() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mini.anathem-pack");
    support::write_pack_mini(&r, "mini-es");
    mem(d.path(), &["save", "hola mundo"]);
    assert!(mem(d.path(), &["model", "install", r.to_str().unwrap()]).status.success());

    let instalado = d.path().join("paquetes").join("mini-es.anathem-pack");
    let firma = d.path().join("paquetes").join("mini-es.anathem-pack.sha256");
    assert!(firma.exists(), "el paquete instalado tiene que quedar verificable");
    assert!(
        std::fs::read_to_string(&firma).unwrap().contains(&anathem::pack::sha256(&instalado).unwrap()),
        "y el sha que dice tiene que ser el del archivo que está"
    );

    let o = mem(d.path(), &["model", "install", instalado.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
