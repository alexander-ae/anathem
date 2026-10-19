//! Deja el commit con el que se compila el binario en `ANATHEM_COMMIT`, para
//! que `mem --version` y `mem doctor` puedan decirlo. Es lo que permite notar
//! un binario instalado que quedó atrás del código.
//!
//! Compilar nunca puede fallar por esto: sin git en el PATH, fuera de un
//! repositorio (un tarball, `cargo publish`) o en un repositorio sin commits,
//! el valor es `desconocido` y la compilación sigue.

use std::path::{Path, PathBuf};
use std::process::Command;

const DESCONOCIDO: &str = "desconocido";

/// Corre `git <args>` en `dir` y devuelve su salida sin el salto de línea
/// final, o `None` si git no está, falla, o no imprime nada.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let salida = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !salida.status.success() {
        return None;
    }
    let texto = String::from_utf8(salida.stdout).ok()?;
    let texto = texto.trim();
    (!texto.is_empty()).then(|| texto.to_string())
}

/// ¿El repositorio de git que contiene a `dir` es el del propio paquete?
///
/// Si el código se compila desde un tarball descomprimido dentro de otro
/// repositorio, `git rev-parse HEAD` contestaría con el commit del repositorio
/// de afuera: un dato falso y con aspecto de verdadero, peor que ninguno.
fn es_la_raiz_del_repositorio(dir: &Path) -> bool {
    let (Some(raiz), Ok(dir)) = (git(dir, &["rev-parse", "--show-toplevel"]), dir.canonicalize())
    else {
        return false;
    };
    Path::new(&raiz).canonicalize().is_ok_and(|r| r == dir)
}

/// Archivos de git que cambian cuando cambia el commit. Sin avisarle a cargo,
/// el commit quedaría congelado en el de la primera compilación, que es justo
/// la falla que `--version` existe para hacer visible.
///
/// `HEAD` y el archivo de la rama no alcanzan: tras un `git gc` o un
/// `git pack-refs` la rama vive solo en `packed-refs`, el siguiente commit
/// crea el archivo suelto, y ni `HEAD` ni `packed-refs` cambian. Lo mismo
/// pasa con el primer commit de un repositorio recién creado. El `reflog` de
/// `HEAD` (`logs/HEAD`) crece en cada commit, checkout, reset o merge, esté
/// donde esté la ref, así que es el que cubre todos los casos; los otros
/// quedan como respaldo para repositorios con el reflog apagado.
fn archivos_que_cambian_con_el_commit(dir: &Path) -> Vec<PathBuf> {
    let ruta = |p: &str| git(dir, &["rev-parse", "--git-path", p]).map(|r| dir.join(r));
    let mut v = Vec::new();
    if let Some(head) = ruta("HEAD") {
        if let Ok(contenido) = std::fs::read_to_string(&head) {
            if let Some(rama) = contenido.trim().strip_prefix("ref: ") {
                v.extend(ruta(rama));
            }
        }
        v.push(head);
    }
    v.extend(ruta("packed-refs"));
    v.extend(ruta("logs/HEAD"));
    v
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let commit = if es_la_raiz_del_repositorio(&dir) {
        let commit = git(&dir, &["rev-parse", "--short", "HEAD"]);
        for f in archivos_que_cambian_con_el_commit(&dir) {
            // Un archivo que no existe haría que cargo recompile siempre, y
            // eso solo se acepta mientras no hay ningún commit: es el único
            // momento en que `logs/HEAD` todavía no existe y es justo el
            // archivo que va a avisar del primero.
            if f.exists() || commit.is_none() {
                println!("cargo:rerun-if-changed={}", f.display());
            }
        }
        commit
    } else {
        None
    };
    println!("cargo:rustc-env=ANATHEM_COMMIT={}", commit.as_deref().unwrap_or(DESCONOCIDO));
}
