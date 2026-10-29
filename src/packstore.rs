//! Identity policy for the active pack.
//!
//! Everything deciding *which* pack a database uses — where its file lives,
//! whether the one on disk is the same one it indexed with, and what to do
//! when it is not — lives here and only here. It used to be spread across
//! `main.rs` and `mcp.rs`: the path `dir/paquetes/{name}.anathem-pack`
//! rebuilt by hand in five places, the sync block copied three times, and
//! the whole policy hidden inside a function called `paquete_activo` that
//! wrote to the database and could abort the command.
//!
//! The two rules that order this module:
//!
//! 1. **With no pack installed, absolutely nothing happens.** No function
//!    here costs more than one query against `meta` when there is no pack.
//! 2. **Degrading to bm25 is acceptable; mixing vectors from two packs never
//!    is.** A mixed index returns plausible, wrong results, the worst
//!    outcome for a memory tool. On any doubt — a rename that fails, a
//!    transaction that does not close — it degrades to bm25, which is a
//!    worse result but an honest one.

use crate::pack::{self, Pack};
use crate::vecstore;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Extension of pack files. The path↔name convention — the pack called `N`
/// lives in `paquetes/N.anathem-pack` — is what makes it possible to find
/// the file knowing only what `meta` says.
pub const EXTENSION: &str = "anathem-pack";

/// How many chunks `install` looks at to decide whether the pack is any use
/// for this memory. Two hundred is enough: if not one of two hundred chunks
/// holds a single word the pack recognises, the two hundred and first will
/// not either.
const MUESTRA: usize = 200;

/// How the database identifies the pack it indexed with: name, sha256 and
/// dimensions. It lives in `vecstore`, which reads and writes it; it is
/// re-exported here because this is the layer that enforces it.
pub use crate::vecstore::PackIdentity;

/// The directory where a database's packs live.
pub fn packs_dir(dir: &Path) -> PathBuf {
    dir.join("paquetes")
}

/// The canonical path of the pack called `name`.
pub fn pack_path(dir: &Path, name: &str) -> PathBuf {
    packs_dir(dir).join(format!("{name}.{EXTENSION}"))
}

/// The result of resolving the active pack. **It is not a `Result`**: the
/// vector layer is not allowed to make a search fail. Everything that goes
/// wrong turns into `pack: None` plus a warning on stderr.
pub struct Resolved {
    pub pack: Option<Pack>,
    /// The identity the database had `pack` registered under when it was
    /// opened. It is `Some` exactly when `pack` is.
    ///
    /// It travels alongside the pack because writing vectors demands it:
    /// `vecstore::sync` has to be able to check, inside the very
    /// transaction it writes in, that it is still the registered one.
    /// Searching does not need it — reading with the wrong pack gives a bad
    /// result, not a mixed index — so `resolve_and_warn` drops it and the
    /// read paths never see it.
    pub identidad: Option<PackIdentity>,
    /// What to tell the user, if there is anything to tell. With no pack
    /// installed it is always `None`: whoever does not use the layer reads
    /// no warnings.
    pub aviso: Option<String>,
}

impl Resolved {
    fn sin_paquete() -> Resolved {
        Resolved { pack: None, identidad: None, aviso: None }
    }

    fn degradado(aviso: String) -> Resolved {
        Resolved { pack: None, identidad: None, aviso: Some(aviso) }
    }

    fn resuelto(pack: Pack, identidad: PackIdentity, aviso: Option<String>) -> Resolved {
        Resolved { pack: Some(pack), identidad: Some(identidad), aviso }
    }

    /// Prints the warning on stderr, if there is one.
    pub fn warn(&self) {
        if let Some(a) = &self.aviso {
            eprintln!("{a}");
        }
    }

    /// The pair needed by whoever is about to **write** vectors. `None`
    /// when there is no usable pack, which is when there is nothing to
    /// write.
    pub fn para_escribir(&self) -> Option<(&Pack, &PackIdentity)> {
        match (&self.pack, &self.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        }
    }
}

/// Opens the registered pack, if any, leaving the database consistent.
///
/// Returns `None` — degrading to bm25 — when no pack is registered, the file
/// is gone, it is corrupt, or reconciliation could not be done: searching
/// can never fail because of the vector layer.
///
/// If the pack on disk is not the one the index registered (a different
/// name, or different dimensions), it reindexes before handing it back. And
/// if it cannot reindex, it does not hand the pack back: searching with the
/// old vectors and the new pack would be exactly the mixture that must
/// never happen.
pub fn resolve(conn: &Connection, dir: &Path) -> Resolved {
    match resolve_inner(conn, dir) {
        Ok(a) => a,
        Err(e) => Resolved::degradado(format!(
            "aviso: no se pudo resolve el paquete de vocabulario ({e}); \
             se busca solo con bm25."
        )),
    }
}

/// `resolve` plus the warning already printed, which is what almost every
/// caller wants.
pub fn resolve_and_warn(conn: &Connection, dir: &Path) -> Option<Pack> {
    let a = resolve(conn, dir);
    a.warn();
    a.pack
}

fn resolve_inner(conn: &Connection, dir: &Path) -> Result<Resolved> {
    let Some((name, sha, dims)) = vecstore::registered_pack(conn)? else {
        // The path for whoever installed nothing: one query and out.
        return Ok(Resolved::sin_paquete());
    };
    // An install may have died between the rename — which already replaced
    // the file — and the `adopt_pack` that was going to describe it. The
    // mark is the only sign of that: the registered sha is discarded on
    // purpose, so if the new build has the same name and the same
    // dimensions as the old one — updating `en-es`, the most common case —
    // the identity guard does not see it.
    //
    // It is only dangerous when it names the **registered** pack: the rename
    // writes to `paquetes/{new_name}`, so if the name is a different one,
    // nobody touched the registered pack's file and the mark was orphaned.
    //
    // Whoever installed nothing does not pay for this query: with no pack
    // registered we already returned above.
    let sospechoso = match vecstore::instalacion_pendiente(conn)? {
        None => false,
        Some(instalando) if instalando == name => true,
        Some(_) => {
            let _ = vecstore::clear_install_mark(conn);
            false
        }
    };
    // The sha is not verified here: hashing 22 MB on every startup would
    // penalise whoever did install the pack. It is verified once, on
    // install.
    let ruta_registrada = pack_path(dir, &name);
    if !ruta_registrada.exists() {
        return Ok(Resolved::degradado(format!(
            "aviso: el paquete {name} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            ruta_registrada.display()
        )));
    }
    let p = match Pack::open_pack(&ruta_registrada) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Resolved::degradado(format!(
                "aviso: el paquete {name} en {} no se pudo abrir ({e}); \
                 se busca solo con bm25. Reinstalalo con `mem model install`.",
                ruta_registrada.display()
            )))
        }
    };
    if !sospechoso && p.name() == name && p.dims() == dims {
        return Ok(Resolved::resuelto(p, (name, sha, dims), None));
    }
    // Reconciling recomputes the sha, adopts again — which deletes the old
    // vectors and the mark in one transaction — and reindexes. For an
    // install interrupted before the rename that is extra work, but it is
    // one-time work on the safe side: the alternative is being unable to
    // tell apart the case where the file did change.
    let aviso = if sospechoso {
        format!(
            "aviso: una instalación de {name} quedó a medias y el archivo en disco puede \
             no ser el que indexó esta base; reindexando para no mezclar vectores."
        )
    } else {
        format!(
            "aviso: el paquete en disco ({}, {} dimensiones) no es el que indexó esta base \
             ({name}, {dims} dimensiones); reindexando para no mezclar vectores.",
            p.name(),
            p.dims()
        )
    };
    match reconcile(conn, dir, &ruta_registrada, p) {
        Ok((p, id)) => Ok(Resolved::resuelto(p, id, Some(aviso))),
        Err(e) => Ok(Resolved::degradado(format!(
            "{aviso}\naviso: no se pudo reindexar ({e}); se busca solo con bm25 \
             hasta que se pueda. Reinstalá el paquete con `mem model install`."
        ))),
    }
}

/// Makes the database describe the pack that is actually on disk again.
///
/// The delicate part is the order. The rename goes **first**, on purpose:
/// if `paquetes/` is read-only — the case that really happens — it fails
/// before touching the database and nothing is left halfway; search degrades
/// to bm25 and recovers on its own once the directory is writable again. If
/// the rename succeeds but the database does not follow, it is undone: the
/// registry did not change, so the path↔name convention has to keep holding
/// for the old name or the next run would never find the file again.
fn reconcile(
    conn: &Connection,
    dir: &Path,
    ruta_actual: &Path,
    p: Pack,
) -> Result<(Pack, PackIdentity)> {
    // The rest of the code assumes the path↔name convention. If the real
    // name (read from the file) is a different one, it has to be renamed as
    // part of reindexing. On POSIX moving the open mmap would be enough —
    // the inode is the same — but Windows does not allow renaming a file
    // with an open memory mapping. That is why `p` is dropped before the
    // rename and reopened afterwards from `target`: on no platform is an
    // mmap left pointing at the old name while the file moves.
    let name = p.name().to_string();
    let dims = p.dims();
    let target = pack_path(dir, &name);
    let hubo_rename = target != ruta_actual;
    let p = if hubo_rename {
        drop(p);
        std::fs::rename(ruta_actual, &target).with_context(|| {
            format!(
                "no se pudo renombrar {} a {} al reindexar",
                ruta_actual.display(),
                target.display()
            )
        })?;
        match Pack::open_pack(&target) {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::rename(&target, ruta_actual);
                return Err(e).with_context(|| {
                    format!(
                        "el rename a {} salió bien pero el paquete no se pudo reabrir",
                        target.display()
                    )
                });
            }
        }
    } else {
        p
    };
    let adopcion = pack::sha256(&target).and_then(|sha| {
        vecstore::adoptar_pack(conn, &name, &sha, dims)?;
        Ok((name.clone(), sha, dims))
    });
    let identidad = match adopcion {
        Ok(i) => i,
        Err(e) => {
            if hubo_rename {
                // `p` has `target` mapped: it has to be dropped before
                // undoing the rename, for the same Windows reason that
                // forced reopening it above.
                drop(p);
                let _ = std::fs::rename(&target, ruta_actual);
            }
            return Err(e);
        }
    };
    // Filling in the vectors is idempotent and sits outside the atomic
    // part: if the process dies here there are fewer vectors than there
    // should be — which the next write completes — but never vectors from
    // two packs.
    //
    // If another process took ownership of the pack in the meantime, this
    // reindex is no longer valid and returning `p` would hand the searcher a
    // pack that is not the registered one. It fails, which in `resolve_inner`
    // means degrading to bm25 this once; the next resolution finds the new
    // pack and starts clean.
    if vecstore::sync(conn, &p, &identidad)?.abandonada() {
        bail!(
            "otro proceso cambió el paquete de vocabulario mientras se reindexaba con {}",
            name
        );
    }
    Ok((p, identidad))
}

/// What `install` reports back to whoever ran it.
pub struct Instalacion {
    pub name: String,
    pub dims: usize,
    pub fragmentos: usize,
    /// `true` if another process changed the registered pack while this
    /// install was indexing: the pack is installed on disk but is no longer
    /// the one the database uses, and the index was left halfway.
    pub desplazada: bool,
}

/// Replaces `target` with the contents of `source`, even when `target`
/// already exists.
///
/// `std::fs::rename` replaces atomically on POSIX when the destination
/// exists, but on Windows it fails in that case. Here it is done in two
/// steps, without relying on the Windows failure reason being recognisable
/// from another platform:
///
/// 1. If `target` is a file, it is set aside under a sibling name in the
///    same directory (same filesystem: the rename that sets it aside and
///    the one that restores it, if needed, are as cheap as the original).
/// 2. `source` is renamed to `target`.
/// 3. If step 2 succeeds, the sibling is deleted. If it fails — antivirus
///    holding the file for an instant, say: a real error, not a hang — the
///    sibling is **restored** to `target` before returning the error, so
///    the caller finds exactly the file that already worked, not a deleted
///    destination with no replacement.
///
/// A first attempt at this helper deleted `target` before renaming instead
/// of setting it aside: if the second step failed, the old file — which had
/// been working until then — was lost for no reason. Detectable
/// (`resolve_inner` sees it missing and degrades to bm25), but a real
/// resilience regression against the atomic rename POSIX used to do and
/// that this helper replaces: losing a file that worked was never
/// necessary.
///
/// When `target` exists but is not a file (a directory, an already corrupt
/// state no install produces) there is nothing worth safeguarding, so it is
/// renamed directly and the error — if any — propagates as is, without
/// opening step 1's window.
///
/// A brief window remains, between step 1 and step 2, in which `target`
/// does not exist. That is acceptable, non-silent degradation: a concurrent
/// search looking right then sees the file is missing (`resolve_inner`
/// checks with `registered_path.exists()`) and falls back to bm25, which is
/// the safe result; and if the process dies in that window, the
/// `pack_instalando` mark already covers the database describing a pack the
/// disk does not have. What can never happen — the database describing one
/// pack while the disk holds another with nothing noticing — does not depend
/// on this window: it is covered by `target` being, at every moment, "the
/// file for `name`" or simply not existing.
fn replace_file(origen: &Path, target: &Path) -> std::io::Result<()> {
    if !target.is_file() {
        return std::fs::rename(origen, target);
    }
    let lateral = target.with_file_name(format!(
        "{}.reemplazando-{}",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("paquete"),
        std::process::id()
    ));
    std::fs::rename(target, &lateral)?;
    match std::fs::rename(origen, target) {
        Ok(()) => {
            // Best effort: if this fails a leftover sibling file is left
            // lying around, not an inconsistent state — `target` already
            // holds the right content.
            let _ = std::fs::remove_file(&lateral);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&lateral, target);
            Err(e)
        }
    }
}

/// Installs a pack from a local file and reindexes.
///
/// It is the only entry point that changes a database's pack, and it is
/// written so that no observable intermediate state matters:
///
/// 1. The sha256 is verified — here and only here — and the pack is checked
///    to open.
/// 2. It is rejected if it would not recognise a single word of this
///    memory.
/// 3. The file is copied to a temporary and only then renamed over the
///    destination, which within the same directory is atomic. That is why
///    installing from the already-installed path — reinstalling "to force a
///    reindex" — works instead of truncating the pack to zero bytes.
/// 4. `pack_instalando` is marked before the rename, because the rename
///    already replaces the file while the registry is written only in the
///    next step: between the two, a SIGKILL would leave the new build on
///    disk with `meta` describing the old one, and with the same name and
///    the same dimensions nothing would give it away. The mark is cleared by
///    `adopt_pack` in its own transaction, so surviving the install means
///    exactly "the file may not be the one `meta` describes" and `resolve`
///    reindexes.
/// 5. Deleting the old vectors and recording the new pack is **a single
///    transaction**: there is no window with one pack's vectors attributed
///    to another.
/// 6. Filling in the vectors stays outside, in batches, and is idempotent: a
///    Ctrl-C midway leaves fewer vectors, never mixed ones.
pub fn install(conn: &Connection, dir: &Path, archivo: &Path) -> Result<Instalacion> {
    let sha = pack::sha256(archivo)?;
    verificar_firma(archivo, &sha)?;

    let (name, dims, sirve) = {
        let p = Pack::open_pack(archivo)?;
        (p.name().to_string(), p.dims(), !solo_vectores_nulos(conn, &p)?)
    };
    if !sirve {
        bail!(
            "el paquete {name} no reconoce ni una palabra de esta memoria: todos los \
             vectores saldrían nulos y la búsqueda vectorial no aportaría nada. No se \
             instaló nada; revisá que sea el paquete del idioma correcto."
        );
    }

    let destino_dir = packs_dir(dir);
    std::fs::create_dir_all(&destino_dir)?;
    let target = pack_path(dir, &name);
    let temporal = destino_dir.join(format!(".instalando-{}.{EXTENSION}", std::process::id()));
    let copia = (|| -> Result<()> {
        std::fs::copy(archivo, &temporal).with_context(|| {
            format!("no se pudo copiar {} a {}", archivo.display(), temporal.display())
        })?;
        // Make sure the copy is the copy: if it came out short or the disk
        // filled up, the sha does not match and the installed pack never
        // comes to exist.
        let sha_copia = pack::sha256(&temporal)?;
        if sha_copia != sha {
            bail!("la copia de {} quedó incompleta; no se instaló nada", archivo.display());
        }
        Pack::open_pack(&temporal)?;
        Ok(())
    })();
    if let Err(e) = copia {
        let _ = std::fs::remove_file(&temporal);
        return Err(e);
    }
    // The mark goes before the rename, not after: written afterwards, the
    // uncovered stretch would be exactly the one meant to be covered. Having
    // it to spare — left set without the file having changed — costs a
    // reindex; having it missing costs a silently mixed index.
    vecstore::mark_install(conn, &name)?;
    if let Err(e) = replace_file(&temporal, &target) {
        let _ = std::fs::remove_file(&temporal);
        // `replace_file` restores `target` to what it held before when the
        // second step fails, so the registered pack's file was not touched
        // (unless the restore *also* failed, an unlikely double failure;
        // there it is simply missing, and `resolve_inner` already detects
        // that without needing the mark — it checks
        // `registered_path.exists()` before looking at it). Leaving the mark
        // set would only cost a useless reindex the next time the name
        // matches the registered one.
        let _ = vecstore::clear_install_mark(conn);
        return Err(anyhow::Error::new(e)
            .context(format!("no se pudo instalar el paquete en {}", target.display())));
    }

    // Clears the mark in the same transaction in which it adopts.
    vecstore::adoptar_pack(conn, &name, &sha, dims)?;
    let p = Pack::open_pack(&target)?;
    // Indexing a large corpus takes minutes and another terminal may
    // install a different pack meanwhile. If that happens, this install is
    // displaced: it is not an error — the pack was installed and the other
    // process is indexing its own — but it has to be said, because the index
    // was left incomplete and `model status` is the only place that shows.
    let s = vecstore::sync(conn, &p, &(name.clone(), sha, dims))?;
    Ok(Instalacion { name, dims, fragmentos: s.escritos(), desplazada: s.abandonada() })
}

/// Removes the pack and its vectors. Returns the name of the one that was
/// there, or `None` if there was none.
///
/// The database first and the file second: if deleting the file fails, the
/// database is already pack-less and search is back to bm25 only, which is
/// what was asked for. The other way round would leave a registry entry
/// pointing at nothing.
pub fn quitar(conn: &Connection, dir: &Path) -> Result<Option<String>> {
    let Some((name, _, _)) = vecstore::registered_pack(conn)? else {
        return Ok(None);
    };
    vecstore::forget_pack(conn)?;
    let _ = std::fs::remove_file(pack_path(dir, &name));
    Ok(Some(name))
}

/// Verifies the `.sha256` travelling beside the pack, if one travels at all.
fn verificar_firma(archivo: &Path, sha: &str) -> Result<()> {
    let firma = archivo.with_file_name(format!(
        "{}.sha256",
        archivo.file_name().unwrap_or_default().to_string_lossy()
    ));
    if !firma.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&firma)?;
    let esperado = raw.split_whitespace().next().unwrap_or("");
    if esperado != sha {
        bail!(
            "el sha256 no coincide: el archivo da {sha} y {} dice {esperado}",
            firma.display()
        );
    }
    Ok(())
}

/// Does the pack produce the null vector for everything already saved?
///
/// That happens when the pack's table does not cover the vocabulary this
/// memory uses: it installs cleanly, reports full coverage and never finds
/// anything, with no signal at all. With an empty memory there is nothing to
/// judge, so it is let through.
fn solo_vectores_nulos(conn: &Connection, p: &Pack) -> Result<bool> {
    let cuerpos: Vec<String> = conn
        .prepare("select body from chunks where trim(body) <> '' limit ?1")?
        .query_map([MUESTRA as i64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if cuerpos.is_empty() {
        return Ok(false);
    }
    for c in &cuerpos {
        if p.embeber(c)?.iter().any(|&x| x != 0.0) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The active pack, revalidated before every operation.
///
/// The MCP server lives for hours. In that time another terminal can run
/// `mem model install` or `mem model remove`, and a server still using the
/// pack it opened at startup would write the old pack's vectors next to the
/// new one's. With different dimensions the short blob would be dead; with
/// the same ones — the real case, since the format fixes 256 for every pack
/// — nothing would tell them apart and search would compute cosines against
/// two mixed vector spaces.
///
/// Revalidating costs one query against `meta`. Reopening — which costs
/// between 40 and 120 ms, plus some 54 MB of tokenizer heap — happens only
/// when the identity really changed. `new` opens nothing: it starts blank
/// (`registry: None, active: None`), on purpose, so a session that never
/// searches or saves never pays that heap. Whoever wants the pack calls
/// `current`, which opens at the moment it is really needed — the first
/// search or the first save — and not before.
/// Warns, **without opening the pack**, when the registry points at a file
/// that is not there. It exists because opening is expensive and deferred
/// until first use: a session that only lists tags would never open
/// anything and would therefore never notice its pack vanished. Checking
/// the file exists costs one `stat`, so that case — the common one — can be
/// caught at startup.
///
/// A pack that exists but is corrupt is still discovered only on use:
/// detecting that requires parsing it, which is precisely what is
/// deferred.
pub fn warn_if_file_missing(conn: &Connection, dir: &Path) {
    let Ok(Some((name, _, _))) = vecstore::registered_pack(conn) else {
        return;
    };
    let r = pack_path(dir, &name);
    if !r.exists() {
        eprintln!(
            "aviso: el paquete {name} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            r.display()
        );
    }
}

pub struct LivePack {
    dir: PathBuf,
    /// What `meta` said the last time it reloaded, whether or not the pack
    /// could be opened, or `None` if it never reloaded yet. It is the
    /// "something changed" (or "not looked at yet") signal: storing it even
    /// when the pack does not open avoids retrying the open — between 40 and
    /// 120 ms — on every message for as long as it stays broken.
    registro: Option<PackIdentity>,
    /// The open pack together with the identity it was opened under. They
    /// are never built separately: that pair is what `sync` needs so it does
    /// not write a stale pack's vectors, and reading `meta` a second time to
    /// complete it would reopen the very window the pair closes.
    activo: Option<(Pack, PackIdentity)>,
    /// When the pack was last used, so it can be released after a long idle
    /// stretch. `None` while none is open.
    last_used: Option<std::time::Instant>,
    /// It was released for being idle and has to be reopened on next use.
    ///
    /// This is needed because `registry` does not change when it is
    /// released: without this flag, `current` would see the registry is the
    /// same, decide there is nothing to reload, and the session would be
    /// left without a vector layer forever. And looking at whether `active`
    /// is empty is not enough: that also happens when no pack is installed
    /// at all, and retrying on every message there would cost between 40 and
    /// 120 ms, which is exactly what `registry` avoids.
    released: bool,
}

/// Asks the allocator to return to the system what it no longer uses.
///
/// Without this, releasing the pack does not show: `drop` frees the
/// tokenizer's ~50 MB for the program, but glibc keeps them in its arena and
/// RSS does not drop. Measured: 64 MB before releasing, 57 after; with this,
/// it returns to the order of the 9 MB it started at.
///
/// It is glibc-specific. On musl, macOS or Windows it does not exist and the
/// call is absent: there, releasing the pack frees the mapping all the same,
/// and the rest is up to the platform's allocator.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn return_heap_to_os() {
    extern "C" {
        fn malloc_trim(pad: usize) -> std::os::raw::c_int;
    }
    // Safe: it touches none of the program's memory, it only tells the
    // allocator to hand back free pages from its arena.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn return_heap_to_os() {}

impl LivePack {
    /// Touches neither the database nor the disk: opening the pack is left
    /// to the first `current()` that really needs it. See the `LivePack`
    /// comment above.
    pub fn new_id(dir: &Path) -> LivePack {
        LivePack {
            dir: dir.to_path_buf(),
            registro: None,
            activo: None,
            last_used: None,
            released: false,
        }
    }

    /// The pack to use right now, with the identity it was opened under.
    ///
    /// Both come out of the same call on purpose: reading them separately
    /// would reopen the window this pair closes, because the registry can
    /// change between one read and the other.
    pub fn actual(&mut self, conn: &Connection) -> Option<(&Pack, &PackIdentity)> {
        let ahora = vecstore::registered_pack(conn).ok().flatten();
        if ahora != self.registro || self.released {
            self.recargar(conn);
        }
        if self.activo.is_some() {
            self.last_used = Some(std::time::Instant::now());
        }
        self.activo.as_ref().map(|(p, i)| (p, i))
    }

    /// Releases the pack when it has gone longer than `limit` unused.
    /// Returns whether it released it.
    ///
    /// The tokenizer costs some 60 MB of private heap per process, and a
    /// session that searched once and then went quiet keeps paying for it
    /// until it closes. Releasing it brings the session back to about 7 MB;
    /// the only cost is reopening it — on the order of 90 ms — on the next
    /// search, which by definition is no longer immediate: there was a gap
    /// in between.
    ///
    /// The `registry` is not forgotten: it tells whether the pack changed,
    /// and dropping it would force rereading `meta` for no reason.
    pub fn release_if_idle(&mut self, limit_: std::time::Duration) -> bool {
        if limit_.is_zero() || self.activo.is_none() {
            return false;
        }
        match self.last_used {
            Some(t) if t.elapsed() >= limit_ => {
                self.activo = None;
                self.last_used = None;
                self.released = true;
                return_heap_to_os();
                true
            }
            _ => false,
        }
    }

    /// Whether a pack is open right now. For tests and diagnostics.
    pub fn has_open_pack(&self) -> bool {
        self.activo.is_some()
    }

    fn recargar(&mut self, conn: &Connection) {
        let a = resolve(conn, &self.dir);
        a.warn();
        // The pair comes whole out of `resolve`, which knows what identity
        // it opened the pack under. Rereading `meta` here to complete it
        // could pair the pack from before with the registry from after.
        self.activo = match (a.pack, a.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        };
        // To detect the next change, though, what `meta` says now is what
        // serves: `resolve` may have reindexed and changed the registry, and
        // storing the earlier one would reload on every message.
        self.registro = vecstore::registered_pack(conn).ok().flatten();
        self.last_used = self.activo.as_ref().map(|_| std::time::Instant::now());
        self.released = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single-piece Unigram tokenizer. These tests do not exercise
    /// embedding, only that the pack opens and that
    /// `reconcile`/`replace_file` move the right file; one piece is
    /// enough.
    const TOKENIZER_MINIMO: &str = r#"{
     "version":"1.0","truncation":null,"padding":null,"added_tokens":[],
     "normalizer":null,
     "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
     "post_processor":null,"decoder":null,
     "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
       "vocab":[["<unk>",0.0]]}
    }"#;

    /// Writes a minimal valid ANPK pack. It duplicates by hand the format
    /// that `tests/apoyo::escribir_pack_filas` already knows how to write,
    /// because that helper lives in the integration crate: this module's
    /// unit tests, which need to reach `reconcile` since it is private,
    /// cannot import it.
    fn escribir_pack_de_prueba(pack_path: &Path, name: &str) {
        let dims = 4usize;
        let fila = [0u8, 0, 0, 0];
        let tok = TOKENIZER_MINIMO.as_bytes();
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"ANPK");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(dims as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes()); // n_tokens
        b.extend_from_slice(&(name.len() as u32).to_le_bytes());
        b.extend_from_slice(name.as_bytes());
        b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
        b.extend_from_slice(tok);
        for _ in 0..dims {
            b.extend_from_slice(&1.0f32.to_le_bytes()); // scales
        }
        for _ in 0..dims {
            b.extend_from_slice(&0.0f32.to_le_bytes()); // mínimos
        }
        b.extend_from_slice(&fila);
        std::fs::write(pack_path, b).unwrap();
    }

    // --- blocker 2: replacing onto a destination that already exists -----
    //
    // `std::fs::rename` atomically replaces an existing destination on
    // POSIX, so the first test in this group already passed with a direct
    // rename. What it proves is that `replace_file` — the new, portable
    // function that sets the destination aside instead of deleting it —
    // does the same: it is precisely the operation (rename onto an occupied
    // destination) that would not be portable to Windows if left as it was.
    // The third test exercises the part an atomic rename never needed:
    // recovering when the second step really fails.

    /// Ningún archivo lateral (`*.reemplazando-*`) sobrevive a un
    /// `reemplazar`, salga bien o mal.
    fn no_stray_files(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .all(|n| !n.contains(".reemplazando-"))
    }

    #[test]
    fn reemplazar_sobre_destino_existente_deja_el_contenido_del_origen() {
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen");
        let target = d.path().join("destino");
        std::fs::write(&origen, b"contenido nuevo").unwrap();
        std::fs::write(&target, b"contenido viejo, mas largo que el nuevo").unwrap();

        replace_file(&origen, &target).unwrap();

        assert!(!origen.exists(), "el origen tiene que haberse movido, no copiado");
        assert_eq!(std::fs::read(&target).unwrap(), b"contenido nuevo");
        assert!(no_stray_files(d.path()), "no puede quedar un archivo lateral sobrante");
    }

    #[test]
    fn reemplazar_sin_destino_previo_tambien_funciona() {
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen");
        let target = d.path().join("destino");
        std::fs::write(&origen, b"contenido").unwrap();

        replace_file(&origen, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"contenido");
    }

    #[test]
    fn reemplazar_si_el_segundo_rename_falla_el_destino_viejo_sigue_sirviendo() {
        // The source does not exist, so the first rename (target ->
        // sibling) succeeds but the second (source -> target) fails with a
        // real error — not a process hang, the same kind of failure an
        // antivirus holding the file for an instant would cause. Before this
        // fix, a `replace_file` reaching this point had already deleted the
        // target: the old file, which had been working until then, was lost
        // for no reason. Now it has to still be there.
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen"); // deliberadamente no se crea
        let target = d.path().join("destino");
        std::fs::write(&target, b"contenido viejo que todavia funciona").unwrap();

        let err = replace_file(&origen, &target).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"contenido viejo que todavia funciona",
            "el destino que funcionaba antes de intentar el reemplazo tiene que seguir ahí"
        );
        assert!(no_stray_files(d.path()), "no puede quedar un archivo lateral sobrante");
    }

    // --- blocker 3: reconciling with the mmap still open -----------------

    #[test]
    fn reconciliar_renombra_un_archivo_con_el_mmap_todavia_abierto() {
        // Reproduces the exact shape of the Windows problem: open the Pack
        // — which maps the file into memory — and hand THAT same `Pack`, in
        // the same process, to `reconcile` so it renames the file it has
        // mapped. It is what `resolve_inner` does in production when the
        // pack on disk is not the registered one.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        std::fs::create_dir_all(packs_dir(dir)).unwrap();
        let ruta_vieja = pack_path(dir, "viejo-es");
        // The file lives under the name "viejo-es" but declares itself
        // "nuevo-es": exactly the case that forces reconcile to rename
        // it.
        escribir_pack_de_prueba(&ruta_vieja, "nuevo-es");

        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        vecstore::register_pack(&conn, "viejo-es", "sha-viejo", 4).unwrap();

        let p = Pack::open_pack(&ruta_vieja).unwrap(); // mmap abierto sobre ruta_vieja

        let (p2, identidad) = reconcile(&conn, dir, &ruta_vieja, p)
            .expect("reconciliar tiene que poder renombrar el archivo que tiene mapeado");

        assert_eq!(identidad.0, "nuevo-es");
        assert_eq!(p2.name(), "nuevo-es");
        assert!(!ruta_vieja.exists(), "el archivo con el nombre viejo tiene que haberse ido");
        assert!(
            pack_path(dir, "nuevo-es").exists(),
            "el archivo tiene que vivir ahora en el nombre nuevo"
        );
    }

    // --- releasing the pack after a while unused -------------------------

    /// Prepares a `LivePack` with the pack already open.
    fn vivo_con_paquete(dir: &Path) -> (rusqlite::Connection, LivePack) {
        std::fs::create_dir_all(packs_dir(dir)).unwrap();
        escribir_pack_de_prueba(&pack_path(dir, "es"), "es");
        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        let sha = crate::pack::sha256(&pack_path(dir, "es")).unwrap();
        vecstore::register_pack(&conn, "es", &sha, 4).unwrap();
        let mut v = LivePack::new_id(dir);
        assert!(v.actual(&conn).is_some(), "el escenario exige el paquete abierto");
        (conn, v)
    }

    #[test]
    fn el_paquete_se_suelta_tras_estar_inactivo_y_vuelve_al_usarlo() {
        let d = tempfile::tempdir().unwrap();
        let (conn, mut v) = vivo_con_paquete(d.path());

        // A deadline that has not elapsed does not release it.
        assert!(!v.release_if_idle(std::time::Duration::from_secs(3600)));
        assert!(v.has_open_pack());

        // One that has elapsed does.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(v.release_if_idle(std::time::Duration::from_millis(10)));
        assert!(!v.has_open_pack(), "tenía que quedar sin paquete");

        // Releasing it again does nothing: there is nothing to release.
        assert!(!v.release_if_idle(std::time::Duration::from_millis(10)));

        // And the next search reopens it on its own.
        let mut v = v;
        assert!(v.actual(&conn).is_some(), "tenía que reabrirse al volver a usarlo");
        assert!(v.has_open_pack());
    }

    #[test]
    fn con_el_plazo_en_cero_no_se_suelta_nunca() {
        let d = tempfile::tempdir().unwrap();
        let (_conn, mut v) = vivo_con_paquete(d.path());
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!v.release_if_idle(std::time::Duration::ZERO));
        assert!(v.has_open_pack(), "cero significa no soltarlo nunca");
    }

    #[test]
    fn usarlo_corre_el_reloj_de_la_inactividad() {
        let d = tempfile::tempdir().unwrap();
        let (conn, mut v) = vivo_con_paquete(d.path());
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Using it right before checking the deadline: it has no longer
        // been idle for 20 ms.
        assert!(v.actual(&conn).is_some());
        assert!(
            !v.release_if_idle(std::time::Duration::from_millis(20)),
            "si se acaba de usar, no está inactivo"
        );
    }
}
