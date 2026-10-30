//! Keeping what anathem writes readable only by whoever owns it.
//!
//! Everything this tool stores is private by nature: the database holds
//! every note the agents saved, and `mem edit`'s temporary holds one of
//! them in the clear while an editor has it open. Created with the default
//! umask both come out `0644`, which on a shared machine means any other
//! local user reads them.
//!
//! On Windows there is no mode to set and the function does nothing: the
//! default ACL on a user profile directory already excludes other users,
//! which is the property being bought here.

use anyhow::{Context, Result};
use std::path::Path;

/// Takes away every permission except the owner's.
///
/// `0600` on a file, `0700` on a directory — the execute bit is what lets a
/// directory be entered at all, so dropping it from a file and keeping it
/// on a directory is the same intent written twice.
///
/// It is **idempotent and applied unconditionally**, not only on creation:
/// a database created by an earlier version is already on disk at `0644`,
/// and a fix that only covered new files would leave exactly the
/// installations that have the most saved in them unprotected.
#[cfg(unix)]
pub fn restringir_a_dueno(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let md = std::fs::metadata(path)
        .with_context(|| format!("no se pudo leer los permisos de {}", path.display()))?;
    let deseado = if md.is_dir() { 0o700 } else { 0o600 };
    // Reading first and only writing on a difference is not an
    // optimisation: on a filesystem that does not carry Unix modes — a
    // mounted exFAT drive, a network share — `set_permissions` fails, and
    // failing there would stop `mem` from opening a database that works
    // perfectly well.
    if md.permissions().mode() & 0o777 == deseado {
        return Ok(());
    }
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(deseado)) {
        eprintln!(
            "aviso: no se pudieron restringir los permisos de {} ({e}); \
             si la máquina es compartida, otros usuarios pueden leerlo",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn restringir_a_dueno(_path: &Path) -> Result<()> {
    Ok(())
}

/// Leaves a file readable by its owner and writable by nobody (`0400`).
///
/// It is for the installed vocabulary pack, and the reason is not secrecy
/// — the pack is a public download — but finding B-2: the pack is held
/// open as a memory mapping for as long as the MCP server lives, and
/// **truncating** a mapped file raises SIGBUS, which kills the process
/// outright with no usable message. Deleting it is harmless, because the
/// inode survives for whoever has it open; it is writing over it in place
/// that is fatal.
///
/// anathem never does that to itself — `install` copies to a temporary and
/// renames, which is why reinstalling the file already in place works —
/// but a stray shell redirection, a half-finished download or another tool
/// writing to that path would. A file with no write bit stops all of them
/// without costing anything: `rename` and `remove_file` need permission on
/// the **directory**, not on the file, so `install` and `remove` go on
/// working exactly as before.
#[cfg(unix)]
pub fn solo_lectura(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let md = std::fs::metadata(path)
        .with_context(|| format!("no se pudo leer los permisos de {}", path.display()))?;
    if md.permissions().mode() & 0o777 == 0o400 {
        return Ok(());
    }
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400)) {
        eprintln!(
            "aviso: no se pudo dejar {} de solo lectura ({e}); \
             escribir encima de un paquete abierto puede tumbar el servidor",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn solo_lectura(_path: &Path) -> Result<()> {
    Ok(())
}
