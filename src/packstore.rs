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
pub fn pack_path(dir: &Path, nombre: &str) -> PathBuf {
    packs_dir(dir).join(format!("{nombre}.{EXTENSION}"))
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
    let Some((nombre, sha, dims)) = vecstore::registered_pack(conn)? else {
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
        Some(instalando) if instalando == nombre => true,
        Some(_) => {
            let _ = vecstore::clear_install_mark(conn);
            false
        }
    };
    // The sha is not verified here: hashing 22 MB on every startup would
    // penalise whoever did install the pack. It is verified once, on
    // install.
    let ruta_registrada = pack_path(dir, &nombre);
    if !ruta_registrada.exists() {
        return Ok(Resolved::degradado(format!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            ruta_registrada.display()
        )));
    }
    let p = match Pack::open_pack(&ruta_registrada) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Resolved::degradado(format!(
                "aviso: el paquete {nombre} en {} no se pudo open_pack ({e}); \
                 se busca solo con bm25. Reinstalalo con `mem model install`.",
                ruta_registrada.display()
            )))
        }
    };
    if !sospechoso && p.nombre() == nombre && p.dims() == dims {
        return Ok(Resolved::resuelto(p, (nombre, sha, dims), None));
    }
    // Reconciling recomputes the sha, adopts again — which deletes the old
    // vectors and the mark in one transaction — and reindexes. For an
    // install interrupted before the rename that is extra work, but it is
    // one-time work on the safe side: the alternative is being unable to
    // tell apart the case where the file did change.
    let aviso = if sospechoso {
        format!(
            "aviso: una instalación de {nombre} quedó a medias y el archivo en disco puede \
             no ser el que indexó esta base; reindexando para no mezclar vectores."
        )
    } else {
        format!(
            "aviso: el paquete en disco ({}, {} dimensiones) no es el que indexó esta base \
             ({nombre}, {dims} dimensiones); reindexando para no mezclar vectores.",
            p.nombre(),
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
    let nombre = p.nombre().to_string();
    let dims = p.dims();
    let target = pack_path(dir, &nombre);
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
        vecstore::adoptar_pack(conn, &nombre, &sha, dims)?;
        Ok((nombre.clone(), sha, dims))
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
            nombre
        );
    }
    Ok((p, identidad))
}

/// What `install` reports back to whoever ran it.
pub struct Instalacion {
    pub nombre: String,
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
            // Best-effort: si esto falla queda un archivo lateral sobrante
            // tirado, no un estado inconsistente — `destino` ya tiene el
            // contenido correcto.
            let _ = std::fs::remove_file(&lateral);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&lateral, target);
            Err(e)
        }
    }
}

/// Instala un paquete desde un archivo local y reindexa.
///
/// Es la única entrada que cambia el paquete de una base, y está escrita
/// para que no exista un estado intermedio observable que importe:
///
/// 1. Se verifica el sha256 —acá y solo acá— y que el paquete abra.
/// 2. Se descarta si no reconocería ni una palabra de esta memoria.
/// 3. El archivo se copia a un temporal y recién ahí se renombra sobre el
///    destino, que dentro del mismo directorio es atómico. Por eso instalar
///    desde la ruta ya instalada —reinstalar "para forzar el reindexado"—
///    funciona en vez de truncar el paquete a cero bytes.
/// 4. Antes del rename se anota `pack_instalando`, porque el rename ya
///    reemplaza el archivo y el registro se escribe recién en el paso
///    siguiente: entre los dos, un SIGKILL dejaría el build nuevo en disco
///    con `meta` describiendo al viejo, y con el mismo nombre y las mismas
///    dimensiones nada lo delataría. La marca la borra `adoptar_pack` en su
///    propia transacción, así que sobrevivir a la instalación significa
///    exactamente "el archivo puede no ser el que `meta` describe" y
///    `resolver` reindexa.
/// 5. Borrar los vectores viejos y anotar el paquete nuevo es **una sola
///    transacción**: no hay ninguna ventana con vectores de un paquete
///    atribuidos a otro.
/// 6. Rellenar los vectores queda afuera, por lotes y es idempotente: un
///    Ctrl-C en el medio deja menos vectores, nunca vectores mezclados.
pub fn install(conn: &Connection, dir: &Path, archivo: &Path) -> Result<Instalacion> {
    let sha = pack::sha256(archivo)?;
    verificar_firma(archivo, &sha)?;

    let (nombre, dims, sirve) = {
        let p = Pack::open_pack(archivo)?;
        (p.nombre().to_string(), p.dims(), !solo_vectores_nulos(conn, &p)?)
    };
    if !sirve {
        bail!(
            "el paquete {nombre} no reconoce ni una palabra de esta memoria: todos los \
             vectores saldrían nulos y la búsqueda vectorial no aportaría nada. No se \
             instaló nada; revisá que sea el paquete del idioma correcto."
        );
    }

    let destino_dir = packs_dir(dir);
    std::fs::create_dir_all(&destino_dir)?;
    let target = pack_path(dir, &nombre);
    let temporal = destino_dir.join(format!(".instalando-{}.{EXTENSION}", std::process::id()));
    let copia = (|| -> Result<()> {
        std::fs::copy(archivo, &temporal).with_context(|| {
            format!("no se pudo copiar {} a {}", archivo.display(), temporal.display())
        })?;
        // Que la copia sea la copia: si quedó corta o el disco se llenó, el
        // sha no da y el paquete instalado no llega a existir.
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
    // La marca va antes del rename, no después: si se escribiera después, el
    // tramo sin cubrir sería justo el que se quiere cubrir. Que sobre —que
    // quede puesta sin que el archivo haya cambiado— cuesta un reindexado;
    // que falte cuesta un índice mezclado en silencio.
    vecstore::mark_install(conn, &nombre)?;
    if let Err(e) = replace_file(&temporal, &target) {
        let _ = std::fs::remove_file(&temporal);
        // `reemplazar` restaura `destino` a lo que tenía antes cuando el
        // segundo paso falla, así que el archivo del paquete registrado no
        // se tocó (a menos que la restauración *también* fallara, un doble
        // fallo improbable; ahí directamente falta, y eso ya lo detecta
        // `resolver_interno` sin necesitar la marca —busca
        // `ruta_registrada.exists()` antes de mirarla—). Dejarla puesta
        // solo costaría un reindexado inútil la próxima vez que el nombre
        // coincida con el registrado.
        let _ = vecstore::clear_install_mark(conn);
        return Err(anyhow::Error::new(e)
            .context(format!("no se pudo instalar el paquete en {}", target.display())));
    }

    // Borra la marca en la misma transacción en la que adopta.
    vecstore::adoptar_pack(conn, &nombre, &sha, dims)?;
    let p = Pack::open_pack(&target)?;
    // Indexar un corpus grande lleva minutos y otra terminal puede instalar
    // otro paquete en el medio. Si pasa, esta instalación queda desplazada:
    // no es un error —el paquete se instaló y el otro proceso está
    // indexando el suyo— pero hay que decirlo, porque el índice quedó
    // incompleto y `model status` es el único lugar donde eso se ve.
    let s = vecstore::sync(conn, &p, &(nombre.clone(), sha, dims))?;
    Ok(Instalacion { nombre, dims, fragmentos: s.escritos(), desplazada: s.abandonada() })
}

/// Quita el paquete y sus vectores. Devuelve el nombre del que había, o
/// `None` si no había ninguno.
///
/// Primero la base y después el archivo: si borrar el archivo falla, la base
/// ya quedó sin paquete y la búsqueda vuelve a ser solo bm25, que es lo que
/// se pidió. Al revés quedaría un registro apuntando a la nada.
pub fn quitar(conn: &Connection, dir: &Path) -> Result<Option<String>> {
    let Some((nombre, _, _)) = vecstore::registered_pack(conn)? else {
        return Ok(None);
    };
    vecstore::forget_pack(conn)?;
    let _ = std::fs::remove_file(pack_path(dir, &nombre));
    Ok(Some(nombre))
}

/// Verifica el `.sha256` que viaja al lado del paquete, si viaja alguno.
fn verificar_firma(archivo: &Path, sha: &str) -> Result<()> {
    let firma = archivo.with_file_name(format!(
        "{}.sha256",
        archivo.file_name().unwrap_or_default().to_string_lossy()
    ));
    if !firma.exists() {
        return Ok(());
    }
    let crudo = std::fs::read_to_string(&firma)?;
    let esperado = crudo.split_whitespace().next().unwrap_or("");
    if esperado != sha {
        bail!(
            "el sha256 no coincide: el archivo da {sha} y {} dice {esperado}",
            firma.display()
        );
    }
    Ok(())
}

/// ¿El paquete produce el vector nulo para todo lo que hay guardado?
///
/// Pasa cuando la tabla del paquete no cubre el vocabulario que usa esta
/// memoria: instala limpio, reporta cobertura completa y no encuentra nada
/// nunca, sin ninguna señal. Con la memoria vacía no hay nada que juzgar y
/// se deja pasar.
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

/// El paquete activo, revalidado antes de cada operación.
///
/// El servidor MCP vive horas. En ese rato otra terminal puede correr
/// `mem model install` o `mem model remove`, y un servidor que siguiera
/// usando el paquete que abrió al arrancar escribiría vectores del paquete
/// viejo junto a los del nuevo. Con dimensiones distintas el blob corto
/// quedaría muerto; con las mismas —el caso real, porque el formato fija 256
/// para todo paquete— nada los distinguiría y la búsqueda calcularía cosenos
/// contra dos espacios vectoriales mezclados.
///
/// Revalidar cuesta una consulta a `meta`. Reabrir —que cuesta entre 40 y
/// 120 ms, y unos 54 MB de heap del tokenizador— pasa solo cuando la
/// identidad cambió de verdad. `nuevo` no abre nada: arranca en blanco
/// (`registro: None, activo: None`), a propósito, para que una sesión que
/// nunca busca ni guarda no pague nunca ese heap. Quien quiera el paquete
/// llama a `actual`, que abre en el momento en que de verdad hace falta —la
/// primera búsqueda o el primer guardado— y no antes.
/// Avisa, **sin abrir el paquete**, si el registro apunta a un archivo que no
/// está. Existe porque abrir es caro y se difiere hasta el primer uso: una
/// sesión que solo lista tags nunca abriría nada y por lo tanto nunca vería
/// que su paquete desapareció. Comprobar que el archivo exista cuesta un
/// `stat`, así que ese caso —el frecuente— se puede detectar al arrancar.
///
/// Un paquete que existe pero está corrupto sigue descubriéndose recién al
/// usarlo: detectarlo exige parsearlo, que es justo lo que se difiere.
pub fn warn_if_file_missing(conn: &Connection, dir: &Path) {
    let Ok(Some((nombre, _, _))) = vecstore::registered_pack(conn) else {
        return;
    };
    let r = pack_path(dir, &nombre);
    if !r.exists() {
        eprintln!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            r.display()
        );
    }
}

pub struct LivePack {
    dir: PathBuf,
    /// Lo que decía `meta` la última vez que se recargó, se haya podido
    /// abrir el paquete o no, o `None` si todavía no se recargó nunca. Es la
    /// señal de "cambió algo" (o "no se miró todavía"): guardarla también
    /// cuando el paquete no abre evita reintentar abrirlo —entre 40 y 120
    /// ms— en cada mensaje mientras siga roto.
    registro: Option<PackIdentity>,
    /// El paquete abierto junto con la identidad con la que se lo abrió.
    /// Nunca se arman por separado: ese par es lo que `sincronizar`
    /// necesita para no escribir vectores de un paquete caducado, y leer
    /// `meta` una segunda vez para completarlo reabriría la misma ventana.
    activo: Option<(Pack, PackIdentity)>,
    /// Cuándo se usó el paquete por última vez, para poder soltarlo si pasa
    /// mucho sin usarse. `None` mientras no haya ninguno abierto.
    last_used: Option<std::time::Instant>,
    /// Se soltó por inactividad y hay que reabrirlo en el próximo uso.
    ///
    /// Hace falta porque `registro` no cambia al soltarlo: sin esta marca,
    /// `actual` vería que el registro es el mismo, decidiría que no hay nada
    /// que recargar, y la sesión se quedaría sin capa vectorial para siempre.
    /// Y no alcanza con mirar si `activo` está vacío: eso también pasa cuando
    /// no hay ningún paquete instalado, y ahí reintentar en cada mensaje
    /// costaría entre 40 y 120 ms, que es justo lo que `registro` evita.
    released: bool,
}

/// Le pide al asignador que devuelva al sistema lo que ya no usa.
///
/// Sin esto, soltar el paquete no se nota: `drop` libera los ~50 MB del
/// tokenizador para el programa, pero glibc los retiene en su arena y el RSS
/// no baja. Medido: 64 MB antes de soltar, 57 después; con esto, vuelve al
/// orden de los 9 MB del arranque.
///
/// Es específico de glibc. En musl, macOS o Windows no existe y la llamada
/// no está: ahí soltar el paquete libera el mapeo igual, y el resto queda a
/// criterio del asignador de la plataforma.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn return_heap_to_os() {
    extern "C" {
        fn malloc_trim(pad: usize) -> std::os::raw::c_int;
    }
    // Seguro: no toca memoria del programa, solo le dice al asignador que
    // devuelva páginas libres de su arena.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn return_heap_to_os() {}

impl LivePack {
    /// No toca la base ni el disco: abrir el paquete queda para el primer
    /// `actual()` que de verdad lo necesite. Ver el comentario de
    /// `PaqueteVivo` arriba.
    pub fn new_id(dir: &Path) -> LivePack {
        LivePack {
            dir: dir.to_path_buf(),
            registro: None,
            activo: None,
            last_used: None,
            released: false,
        }
    }

    /// El paquete que corresponde usar ahora mismo, con la identidad bajo
    /// la que se lo abrió.
    ///
    /// Los dos salen juntos de la misma llamada a propósito: leerlos por
    /// separado reabriría la ventana que este par cierra, porque entre una
    /// lectura y la otra el registro puede cambiar.
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

    /// Suelta el paquete si lleva más de `limite` sin usarse. Devuelve si lo
    /// soltó.
    ///
    /// El tokenizador cuesta unos 60 MB de heap privado por proceso, y una
    /// sesión que buscó una vez y después se quedó quieta los sigue pagando
    /// hasta cerrarse. Soltarlo la devuelve a unos 7 MB; lo único que cuesta
    /// es volver a abrirlo —del orden de 90 ms— en la próxima búsqueda, que
    /// ya no es inmediata por definición: hubo un hueco de por medio.
    ///
    /// No se olvida el `registro`: sirve para saber si el paquete cambió, y
    /// tirarlo obligaría a releer `meta` sin motivo.
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

    /// Si hay un paquete abierto ahora mismo. Para tests y diagnóstico.
    pub fn has_open_pack(&self) -> bool {
        self.activo.is_some()
    }

    fn recargar(&mut self, conn: &Connection) {
        let a = resolve(conn, &self.dir);
        a.warn();
        // El par sale entero de `resolver`, que sabe con qué identidad abrió
        // el paquete. Releer `meta` acá para completarlo podría emparejar el
        // paquete de antes con el registro de después.
        self.activo = match (a.pack, a.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        };
        // Para detectar el próximo cambio, en cambio, sirve lo que dice
        // `meta` ahora: `resolver` puede haber reindexado y cambiado el
        // registro, y si se guardara el de antes se recargaría en cada
        // mensaje.
        self.registro = vecstore::registered_pack(conn).ok().flatten();
        self.last_used = self.activo.as_ref().map(|_| std::time::Instant::now());
        self.released = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokenizador Unigram de una sola pieza. Estos tests no prueban
    /// `embeber`, solo que el paquete abre y que `reconciliar`/`reemplazar`
    /// mueven el archivo correcto; una pieza alcanza.
    const TOKENIZER_MINIMO: &str = r#"{
     "version":"1.0","truncation":null,"padding":null,"added_tokens":[],
     "normalizer":null,
     "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
     "post_processor":null,"decoder":null,
     "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
       "vocab":[["<unk>",0.0]]}
    }"#;

    /// Escribe un paquete ANPK mínimo válido. Duplica a mano el formato que
    /// `tests/apoyo::escribir_pack_filas` ya sabe escribir porque ese
    /// helper vive en el crate de integración: los tests unitarios de este
    /// módulo, que necesitan tocar `reconciliar` por ser privada, no pueden
    /// importarlo.
    fn escribir_pack_de_prueba(pack_path: &Path, nombre: &str) {
        let dims = 4usize;
        let fila = [0u8, 0, 0, 0];
        let tok = TOKENIZER_MINIMO.as_bytes();
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"ANPK");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(dims as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes()); // n_tokens
        b.extend_from_slice(&(nombre.len() as u32).to_le_bytes());
        b.extend_from_slice(nombre.as_bytes());
        b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
        b.extend_from_slice(tok);
        for _ in 0..dims {
            b.extend_from_slice(&1.0f32.to_le_bytes()); // escalas
        }
        for _ in 0..dims {
            b.extend_from_slice(&0.0f32.to_le_bytes()); // mínimos
        }
        b.extend_from_slice(&fila);
        std::fs::write(pack_path, b).unwrap();
    }

    // --- bloqueo 2: reemplazar sobre un destino que ya existe -------------
    //
    // `std::fs::rename` reemplaza atómicamente un destino existente en
    // POSIX, así que el primer test de este grupo ya pasaba con un rename
    // directo. Lo que prueba es que `reemplazar` —la función nueva,
    // portable, que aparta el destino en vez de borrarlo— hace lo mismo: es
    // justo la operación (rename sobre un destino ya ocupado) que hoy no
    // sería portable a Windows si se dejara como estaba. El tercer test
    // ejercita la parte que un rename atómico nunca necesitó: recuperarse
    // cuando el segundo paso falla de verdad.

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
        // El origen no existe, así que el primer rename (destino -> lateral)
        // sale bien pero el segundo (origen -> destino) falla con un error
        // real — no un cuelgue del proceso, el mismo tipo de fallo que
        // dejaría un antivirus reteniendo el archivo un instante. Antes de
        // este arreglo, un `reemplazar` que llegara hasta acá ya había
        // borrado el destino: el archivo viejo, que hasta ese momento
        // andaba, se perdía sin necesidad. Ahora tiene que seguir estando.
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

    // --- bloqueo 3: reconciliar con el mmap todavía abierto ----------------

    #[test]
    fn reconciliar_renombra_un_archivo_con_el_mmap_todavia_abierto() {
        // Reproduce la forma exacta del problema de Windows: abrir el Pack
        // —que mapea el archivo en memoria— y pasar ESE mismo `Pack`, en el
        // mismo proceso, a `reconciliar` para que renombre el archivo que
        // tiene mapeado. Es lo que hace `resolver_interno` en producción
        // cuando el paquete en disco no es el registrado.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        std::fs::create_dir_all(packs_dir(dir)).unwrap();
        let ruta_vieja = pack_path(dir, "viejo-es");
        // El archivo vive bajo el nombre "viejo-es" pero declara ser
        // "nuevo-es": exactamente el caso que obliga a reconciliar a
        // renombrarlo.
        escribir_pack_de_prueba(&ruta_vieja, "nuevo-es");

        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        vecstore::register_pack(&conn, "viejo-es", "sha-viejo", 4).unwrap();

        let p = Pack::open_pack(&ruta_vieja).unwrap(); // mmap abierto sobre ruta_vieja

        let (p2, identidad) = reconcile(&conn, dir, &ruta_vieja, p)
            .expect("reconciliar tiene que poder renombrar el archivo que tiene mapeado");

        assert_eq!(identidad.0, "nuevo-es");
        assert_eq!(p2.nombre(), "nuevo-es");
        assert!(!ruta_vieja.exists(), "el archivo con el nombre viejo tiene que haberse ido");
        assert!(
            pack_path(dir, "nuevo-es").exists(),
            "el archivo tiene que vivir ahora en el nombre nuevo"
        );
    }

    // --- soltar el paquete tras un rato sin usarlo -----------------------

    /// Prepara un `PaqueteVivo` con el paquete ya abierto.
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

        // Un plazo que todavía no venció no lo suelta.
        assert!(!v.release_if_idle(std::time::Duration::from_secs(3600)));
        assert!(v.has_open_pack());

        // Uno ya vencido sí.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(v.release_if_idle(std::time::Duration::from_millis(10)));
        assert!(!v.has_open_pack(), "tenía que quedar sin paquete");

        // Soltarlo de nuevo no hace nada: no hay qué soltar.
        assert!(!v.release_if_idle(std::time::Duration::from_millis(10)));

        // Y la próxima búsqueda lo reabre sola.
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
        // Usarlo justo antes de mirar el plazo: ya no lleva 20 ms quieto.
        assert!(v.actual(&conn).is_some());
        assert!(
            !v.release_if_idle(std::time::Duration::from_millis(20)),
            "si se acaba de usar, no está inactivo"
        );
    }
}
