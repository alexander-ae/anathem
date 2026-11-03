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
const SAMPLE: usize = 200;

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
    pub identity: Option<PackIdentity>,
    /// What to tell the user, if there is anything to tell. With no pack
    /// installed it is always `None`: whoever does not use the layer reads
    /// no warnings.
    pub warning: Option<String>,
}

impl Resolved {
    fn no_pack() -> Resolved {
        Resolved { pack: None, identity: None, warning: None }
    }

    fn degraded(warning: String) -> Resolved {
        Resolved { pack: None, identity: None, warning: Some(warning) }
    }

    fn with_pack(pack: Pack, identity: PackIdentity, warning: Option<String>) -> Resolved {
        Resolved { pack: Some(pack), identity: Some(identity), warning }
    }

    /// Prints the warning on stderr, if there is one.
    pub fn warn(&self) {
        if let Some(a) = &self.warning {
            eprintln!("{a}");
        }
    }

    /// The pair needed by whoever is about to **write** vectors. `None`
    /// when there is no usable pack, which is when there is nothing to
    /// write.
    pub fn for_writing(&self) -> Option<(&Pack, &PackIdentity)> {
        match (&self.pack, &self.identity) {
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
        Err(e) => Resolved::degraded(format!(
            "warning: could not resolve the vocabulary pack ({e}); \
             searching with bm25 only."
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
        return Ok(Resolved::no_pack());
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
    let suspect = match vecstore::pending_install(conn)? {
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
    let registered_path = pack_path(dir, &name);
    if !registered_path.exists() {
        return Ok(Resolved::degraded(format!(
            "warning: pack {name} is registered but {} is missing; \
             searching with bm25 only. Reinstall it with `mem model install`.",
            registered_path.display()
        )));
    }
    let p = match Pack::open_pack(&registered_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Resolved::degraded(format!(
                "warning: pack {name} at {} could not be opened ({e}); \
                 searching with bm25 only. Reinstall it with `mem model install`.",
                registered_path.display()
            )))
        }
    };
    if !suspect && p.name() == name && p.dims() == dims {
        return Ok(Resolved::with_pack(p, (name, sha, dims), None));
    }
    // Reconciling recomputes the sha, adopts again — which deletes the old
    // vectors and the mark in one transaction — and reindexes. For an
    // install interrupted before the rename that is extra work, but it is
    // one-time work on the safe side: the alternative is being unable to
    // tell apart the case where the file did change.
    let warning = if suspect {
        format!(
            "warning: an install of {name} was left half done and the file on disk may \
             not be the one this database indexed with; reindexing so vectors are not mixed."
        )
    } else {
        format!(
            "warning: the pack on disk ({}, {} dimensions) is not the one this database indexed \
             with ({name}, {dims} dimensions); reindexing so vectors are not mixed.",
            p.name(),
            p.dims()
        )
    };
    match reconcile(conn, dir, &registered_path, p) {
        Ok((p, id)) => Ok(Resolved::with_pack(p, id, Some(warning))),
        Err(e) => Ok(Resolved::degraded(format!(
            "{warning}\nwarning: could not reindex ({e}); searching with bm25 only \
             until it can. Reinstall the pack with `mem model install`."
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
    current_path: &Path,
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
    let hubo_rename = target != current_path;
    let p = if hubo_rename {
        drop(p);
        std::fs::rename(current_path, &target).with_context(|| {
            format!(
                "could not rename {} to {} while reindexing",
                current_path.display(),
                target.display()
            )
        })?;
        match Pack::open_pack(&target) {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::rename(&target, current_path);
                return Err(e).with_context(|| {
                    format!(
                        "the rename to {} succeeded but the pack could not be reopened",
                        target.display()
                    )
                });
            }
        }
    } else {
        p
    };
    let adoption = pack::sha256(&target).and_then(|sha| {
        vecstore::adopt_pack(conn, &name, &sha, dims)?;
        Ok((name.clone(), sha, dims))
    });
    let identity = match adoption {
        Ok(i) => i,
        Err(e) => {
            if hubo_rename {
                // `p` has `target` mapped: it has to be dropped before
                // undoing the rename, for the same Windows reason that
                // forced reopening it above.
                drop(p);
                let _ = std::fs::rename(&target, current_path);
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
    if vecstore::sync(conn, &p, &identity)?.abandoned() {
        bail!(
            "another process changed the vocabulary pack while reindexing with {}",
            name
        );
    }
    Ok((p, identity))
}

/// What `install` reports back to whoever ran it.
pub struct Installed {
    pub name: String,
    pub dims: usize,
    pub chunks: usize,
    /// `true` if another process changed the registered pack while this
    /// install was indexing: the pack is installed on disk but is no longer
    /// the one the database uses, and the index was left halfway.
    pub displaced: bool,
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
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if !target.is_file() {
        return std::fs::rename(source, target);
    }
    let sibling = target.with_file_name(format!(
        "{}.reemplazando-{}",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("paquete"),
        std::process::id()
    ));
    std::fs::rename(target, &sibling)?;
    match std::fs::rename(source, target) {
        Ok(()) => {
            // Best effort: if this fails a leftover sibling file is left
            // lying around, not an inconsistent state — `target` already
            // holds the right content.
            let _ = std::fs::remove_file(&sibling);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&sibling, target);
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
pub fn install(conn: &Connection, dir: &Path, file: &Path) -> Result<Installed> {
    let sha = pack::sha256(file)?;
    verify_signature(file, &sha)?;

    let (name, dims, useful) = {
        let p = Pack::open_pack(file)?;
        (p.name().to_string(), p.dims(), !only_null_vectors(conn, &p)?)
    };
    if !useful {
        bail!(
            "pack {name} does not recognise a single word of this memory: every vector would \
             come out null and vector search would add nothing. Nothing was installed; \
             check that it is the pack for the right language."
        );
    }

    let target_dir = packs_dir(dir);
    std::fs::create_dir_all(&target_dir)?;
    let target = pack_path(dir, &name);
    let temp = target_dir.join(format!(".instalando-{}.{EXTENSION}", std::process::id()));
    let copy = (|| -> Result<()> {
        std::fs::copy(file, &temp).with_context(|| {
            format!("could not copy {} to {}", file.display(), temp.display())
        })?;
        // Make sure the copy is the copy: if it came out short or the disk
        // filled up, the sha does not match and the installed pack never
        // comes to exist.
        let sha_copia = pack::sha256(&temp)?;
        if sha_copia != sha {
            bail!("the copy of {} came out incomplete; nothing was installed", file.display());
        }
        Pack::open_pack(&temp)?;
        Ok(())
    })();
    if let Err(e) = copy {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // The mark goes before the rename, not after: written afterwards, the
    // uncovered stretch would be exactly the one meant to be covered. Having
    // it to spare — left set without the file having changed — costs a
    // reindex; having it missing costs a silently mixed index.
    vecstore::mark_install(conn, &name)?;
    if let Err(e) = replace_file(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
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
            .context(format!("could not install the pack at {}", target.display())));
    }

    // Read-only from here on (finding B-2): the pack stays memory-mapped
    // for as long as a server lives, and truncating a mapped file raises
    // SIGBUS. Nothing in anathem writes over it in place, but a stray `>`
    // or a half-finished download would. It does not get in the way of the
    // next install: `rename` needs permission on the directory, not on the
    // file. See `permissions::read_only`.
    let _ = crate::permissions::read_only(&target);

    // Clears the mark in the same transaction in which it adopts.
    vecstore::adopt_pack(conn, &name, &sha, dims)?;
    let p = Pack::open_pack(&target)?;
    // Indexing a large corpus takes minutes and another terminal may
    // install a different pack meanwhile. If that happens, this install is
    // displaced: it is not an error — the pack was installed and the other
    // process is indexing its own — but it has to be said, because the index
    // was left incomplete and `model status` is the only place that shows.
    let s = vecstore::sync(conn, &p, &(name.clone(), sha, dims))?;
    Ok(Installed { name, dims, chunks: s.written(), displaced: s.abandoned() })
}

/// Removes the pack and its vectors. Returns the name of the one that was
/// there, or `None` if there was none.
///
/// The database first and the file second: if deleting the file fails, the
/// database is already pack-less and search is back to bm25 only, which is
/// what was asked for. The other way round would leave a registry entry
/// pointing at nothing.
pub fn remove_pack(conn: &Connection, dir: &Path) -> Result<Option<String>> {
    let Some((name, _, _)) = vecstore::registered_pack(conn)? else {
        return Ok(None);
    };
    vecstore::forget_pack(conn)?;
    let _ = std::fs::remove_file(pack_path(dir, &name));
    Ok(Some(name))
}

/// Verifies the `.sha256` travelling beside the pack, if one travels at all.
///
/// A missing sidecar is not an error here, and that is deliberate: `mem
/// model install` is also how a pack built locally with
/// `tools/build_pack.py` gets installed, and that one has no sidecar
/// to check against. What `install` **does** guarantee on its own is that
/// the copy it puts in place is byte-for-byte the file it was handed (it
/// re-hashes the copy), so the gap is only in "is this the pack the project
/// published", which no local check can answer anyway.
///
/// Closing that gap properly needs a signature — a key published somewhere
/// other than the artifact — and that is a decision with key custody
/// attached, tracked separately in finding M-3. The installer, which is
/// where a pack arrives from the network, now refuses to proceed when it
/// cannot verify (`scripts/install.sh`); this path is the local one.
fn verify_signature(file: &Path, sha: &str) -> Result<()> {
    let firma = file.with_file_name(format!(
        "{}.sha256",
        file.file_name().unwrap_or_default().to_string_lossy()
    ));
    if !firma.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&firma)?;
    let expected = raw.split_whitespace().next().unwrap_or("");
    if expected != sha {
        bail!(
            "the sha256 does not match: the file gives {sha} and {} says {expected}",
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
fn only_null_vectors(conn: &Connection, p: &Pack) -> Result<bool> {
    let bodies: Vec<String> = conn
        .prepare("select body from chunks where trim(body) <> '' limit ?1")?
        .query_map([SAMPLE as i64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if bodies.is_empty() {
        return Ok(false);
    }
    for c in &bodies {
        if p.embed(c)?.iter().any(|&x| x != 0.0) {
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
            "warning: pack {name} is registered but {} is missing; \
             searching with bm25 only. Reinstall it with `mem model install`.",
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
    active: Option<(Pack, PackIdentity)>,
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
            active: None,
            last_used: None,
            released: false,
        }
    }

    /// The pack to use right now, with the identity it was opened under.
    ///
    /// Both come out of the same call on purpose: reading them separately
    /// would reopen the window this pair closes, because the registry can
    /// change between one read and the other.
    pub fn current(&mut self, conn: &Connection) -> Option<(&Pack, &PackIdentity)> {
        let ahora = vecstore::registered_pack(conn).ok().flatten();
        if ahora != self.registro || self.released {
            self.reload(conn);
        }
        if self.active.is_some() {
            self.last_used = Some(std::time::Instant::now());
        }
        self.active.as_ref().map(|(p, i)| (p, i))
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
    pub fn release_if_idle(&mut self, limit: std::time::Duration) -> bool {
        if limit.is_zero() || self.active.is_none() {
            return false;
        }
        match self.last_used {
            Some(t) if t.elapsed() >= limit => {
                self.active = None;
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
        self.active.is_some()
    }

    fn reload(&mut self, conn: &Connection) {
        let a = resolve(conn, &self.dir);
        a.warn();
        // The pair comes whole out of `resolve`, which knows what identity
        // it opened the pack under. Rereading `meta` here to complete it
        // could pair the pack from before with the registry from after.
        self.active = match (a.pack, a.identity) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        };
        // To detect the next change, though, what `meta` says now is what
        // serves: `resolve` may have reindexed and changed the registry, and
        // storing the earlier one would reload on every message.
        self.registro = vecstore::registered_pack(conn).ok().flatten();
        self.last_used = self.active.as_ref().map(|_| std::time::Instant::now());
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
    const MINIMAL_TOKENIZER: &str = r#"{
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
    fn write_test_pack(pack_path: &Path, name: &str) {
        let dims = 4usize;
        let row = [0u8, 0, 0, 0];
        let tok = MINIMAL_TOKENIZER.as_bytes();
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
        b.extend_from_slice(&row);
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

    /// Ningún archivo sibling (`*.reemplazando-*`) sobrevive a un
    /// `reemplazar`, salga bien o mal.
    fn no_stray_files(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .all(|n| !n.contains(".reemplazando-"))
    }

    #[test]
    fn replacing_an_existing_target_leaves_the_source_content() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("origen");
        let target = d.path().join("destino");
        std::fs::write(&source, b"contenido nuevo").unwrap();
        std::fs::write(&target, b"contenido viejo, mas largo que el nuevo").unwrap();

        replace_file(&source, &target).unwrap();

        assert!(!source.exists(), "the source has to have moved, not been copied");
        assert_eq!(std::fs::read(&target).unwrap(), b"contenido nuevo");
        assert!(no_stray_files(d.path()), "no leftover sibling file may remain");
    }

    #[test]
    fn replacing_with_no_previous_target_works_too() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("origen");
        let target = d.path().join("destino");
        std::fs::write(&source, b"contenido").unwrap();

        replace_file(&source, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"contenido");
    }

    #[test]
    fn if_the_second_rename_fails_the_old_target_still_works() {
        // The source does not exist, so the first rename (target ->
        // sibling) succeeds but the second (source -> target) fails with a
        // real error — not a process hang, the same kind of failure an
        // antivirus holding the file for an instant would cause. Before this
        // fix, a `replace_file` reaching this point had already deleted the
        // target: the old file, which had been working until then, was lost
        // for no reason. Now it has to still be there.
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("origen"); // deliberadamente no se crea
        let target = d.path().join("destino");
        std::fs::write(&target, b"contenido viejo que todavia funciona").unwrap();

        let err = replace_file(&source, &target).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"contenido viejo que todavia funciona",
            "the destination that worked before the replacement attempt has to still be there"
        );
        assert!(no_stray_files(d.path()), "no leftover sibling file may remain");
    }

    // --- blocker 3: reconciling with the mmap still open -----------------

    #[test]
    fn reconcile_renames_a_file_with_the_mmap_still_open() {
        // Reproduces the exact shape of the Windows problem: open the Pack
        // — which maps the file into memory — and hand THAT same `Pack`, in
        // the same process, to `reconcile` so it renames the file it has
        // mapped. It is what `resolve_inner` does in production when the
        // pack on disk is not the registered one.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        std::fs::create_dir_all(packs_dir(dir)).unwrap();
        let old_path = pack_path(dir, "viejo-es");
        // The file lives under the name "viejo-es" but declares itself
        // "nuevo-es": exactly the case that forces reconcile to rename
        // it.
        write_test_pack(&old_path, "nuevo-es");

        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        vecstore::register_pack(&conn, "viejo-es", "sha-viejo", 4).unwrap();

        let p = Pack::open_pack(&old_path).unwrap(); // mmap abierto sobre ruta_vieja

        let (p2, identity) = reconcile(&conn, dir, &old_path, p)
            .expect("reconciling has to be able to rename the file it has mapped");

        assert_eq!(identity.0, "nuevo-es");
        assert_eq!(p2.name(), "nuevo-es");
        assert!(!old_path.exists(), "the file under the old name has to be gone");
        assert!(
            pack_path(dir, "nuevo-es").exists(),
            "the file has to live under the new name now"
        );
    }

    // --- releasing the pack after a while unused -------------------------

    /// Prepares a `LivePack` with the pack already open.
    fn live_pack_with_a_pack(dir: &Path) -> (rusqlite::Connection, LivePack) {
        std::fs::create_dir_all(packs_dir(dir)).unwrap();
        write_test_pack(&pack_path(dir, "es"), "es");
        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        let sha = crate::pack::sha256(&pack_path(dir, "es")).unwrap();
        vecstore::register_pack(&conn, "es", &sha, 4).unwrap();
        let mut v = LivePack::new_id(dir);
        assert!(v.current(&conn).is_some(), "the scenario requires the pack to be open");
        (conn, v)
    }

    #[test]
    fn the_pack_is_released_when_idle_and_comes_back_on_use() {
        let d = tempfile::tempdir().unwrap();
        let (conn, mut v) = live_pack_with_a_pack(d.path());

        // A deadline that has not elapsed does not release it.
        assert!(!v.release_if_idle(std::time::Duration::from_secs(3600)));
        assert!(v.has_open_pack());

        // One that has elapsed does.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(v.release_if_idle(std::time::Duration::from_millis(10)));
        assert!(!v.has_open_pack(), "it had to be left with no pack");

        // Releasing it again does nothing: there is nothing to release.
        assert!(!v.release_if_idle(std::time::Duration::from_millis(10)));

        // And the next search reopens it on its own.
        let mut v = v;
        assert!(v.current(&conn).is_some(), "it had to reopen when used again");
        assert!(v.has_open_pack());
    }

    #[test]
    fn with_the_deadline_at_zero_it_is_never_released() {
        let d = tempfile::tempdir().unwrap();
        let (_conn, mut v) = live_pack_with_a_pack(d.path());
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!v.release_if_idle(std::time::Duration::ZERO));
        assert!(v.has_open_pack(), "cero significa no soltarlo nunca");
    }

    #[test]
    fn using_it_resets_the_idle_clock() {
        let d = tempfile::tempdir().unwrap();
        let (conn, mut v) = live_pack_with_a_pack(d.path());
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Using it right before checking the deadline: it has no longer
        // been idle for 20 ms.
        assert!(v.current(&conn).is_some());
        assert!(
            !v.release_if_idle(std::time::Duration::from_millis(20)),
            "if it was just used, it is not idle"
        );
    }
}
