//! De qué proyecto es una memoria, cuando nadie lo dice.
//!
//! Sin esto, todo lo que se guarda sin `--project` cae en `sin-proyecto` y el
//! filtro por proyecto deja de servir — que es exactamente el defecto que
//! tenía el sistema anterior, donde los 390 documentos vivían en un único
//! contenedor de relleno.

use std::path::{Path, PathBuf};

pub const SIN_PROYECTO: &str = "sin-proyecto";

/// Deriva el nombre del proyecto a partir del directorio de trabajo.
///
/// Prioriza el **nombre del remoto** por sobre el del directorio, y no es un
/// capricho: un repositorio suele estar clonado en una carpeta que no se
/// llama como él —`api-monolito` vive en `API/Web`— y dentro de un
/// worktree el directorio se llama como la rama, no como el proyecto. El
/// remoto es el único nombre estable entre clones, worktrees y máquinas.
///
/// No invoca a `git`: lee `.git` directamente. Así funciona aunque git no
/// esté instalado, no cuesta un subproceso, y no depende de que el binario
/// herede un PATH razonable.
pub fn detectar(cwd: &Path) -> Option<String> {
    let comun = dir_git_comun(cwd)?;
    if let Some(n) = nombre_del_remoto(&comun.join("config")) {
        return Some(n);
    }
    // Sin remoto, el nombre de la carpeta que contiene el `.git` común. Es el
    // repositorio principal incluso si estamos parados en un worktree.
    comun.parent()?.file_name()?.to_str().map(str::to_string)
}

/// El directorio `.git` **común**: el del repositorio principal, no el del
/// worktree. En un worktree, `.git` es un archivo que apunta a
/// `<principal>/.git/worktrees/<nombre>`, y ahí adentro hay un archivo
/// `commondir` con la ruta relativa de vuelta al `.git` de verdad.
fn dir_git_comun(cwd: &Path) -> Option<PathBuf> {
    let mut d = cwd;
    loop {
        let punto_git = d.join(".git");
        if punto_git.is_dir() {
            return Some(punto_git);
        }
        if punto_git.is_file() {
            let contenido = std::fs::read_to_string(&punto_git).ok()?;
            let ruta = contenido.strip_prefix("gitdir:")?.trim();
            let gitdir = if Path::new(ruta).is_absolute() {
                PathBuf::from(ruta)
            } else {
                d.join(ruta)
            };
            let commondir = gitdir.join("commondir");
            if let Ok(rel) = std::fs::read_to_string(&commondir) {
                let rel = rel.trim();
                let c = if Path::new(rel).is_absolute() {
                    PathBuf::from(rel)
                } else {
                    gitdir.join(rel)
                };
                return normalizar(&c);
            }
            return normalizar(&gitdir);
        }
        d = d.parent()?;
    }
}

/// Resuelve los `..` de una ruta sin tocar el disco. `commondir` suele ser
/// `../..`, y dejarlo sin resolver haría que `parent()` devolviera el
/// componente equivocado al buscar el nombre de la carpeta.
fn normalizar(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            otro => out.push(otro.as_os_str()),
        }
    }
    Some(out)
}

/// Extrae el nombre del repositorio de la url de `origin` en un config de
/// git. Soporta las dos formas usuales: `git@host:grupo/repo.git` y
/// `https://host/grupo/repo.git`.
fn nombre_del_remoto(config: &Path) -> Option<String> {
    let texto = std::fs::read_to_string(config).ok()?;
    let mut en_origin = false;
    for linea in texto.lines() {
        let l = linea.trim();
        if l.starts_with('[') {
            en_origin = l.replace(char::is_whitespace, "") == "[remote\"origin\"]";
            continue;
        }
        if !en_origin {
            continue;
        }
        if let Some(url) = l.strip_prefix("url") {
            let url = url.trim_start().strip_prefix('=')?.trim();
            let ultimo = url.rsplit(['/', ':']).next()?;
            let nombre = ultimo.strip_suffix(".git").unwrap_or(ultimo);
            if !nombre.is_empty() {
                return Some(nombre.to_string());
            }
        }
    }
    None
}

/// El proyecto a usar: el que pidió quien llama, o el detectado, o el
/// genérico. Un `--project` explícito siempre gana.
pub fn resolver(pedido: Option<String>, cwd: &Path) -> String {
    pedido.unwrap_or_else(|| detectar(cwd).unwrap_or_else(|| SIN_PROYECTO.to_string()))
}
