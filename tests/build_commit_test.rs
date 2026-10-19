//! `build.rs` tiene que mantener el commit al día: si se congela en el de la
//! primera compilación, `mem --version` dice un commit que ya no es el del
//! binario, que es exactamente la falla que existe para hacer visible.
//!
//! Compila un paquete mínimo con una copia del `build.rs` real dentro de un
//! repositorio temporal, y lo recompila tras cada cambio de commit.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=a@b.c", "-c", "user.name=prueba", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git tiene que estar disponible");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Compila y corre el paquete mínimo; devuelve el commit que cree tener.
fn commit_del_binario(dir: &Path, destino: &Path) -> String {
    let o = Command::new(env!("CARGO"))
        .args(["run", "-q", "--offline"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", destino)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn hay_git() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn el_commit_del_binario_sigue_a_head_tras_pack_refs_y_en_el_primer_commit() {
    if !hay_git() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let paquete = d.path().join("mini");
    let destino = d.path().join("target");
    std::fs::create_dir_all(paquete.join("src")).unwrap();
    std::fs::write(
        paquete.join("Cargo.toml"),
        "[package]\nname = \"mini\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(paquete.join("build.rs"), include_str!("../build.rs")).unwrap();
    std::fs::write(
        paquete.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"ANATHEM_COMMIT\")); }\n",
    )
    .unwrap();
    std::fs::write(paquete.join(".gitignore"), "target\n").unwrap();
    git(&paquete, &["init", "-q"]);

    // Sin ningún commit no hay nada que decir, y no puede fallar.
    assert_eq!(commit_del_binario(&paquete, &destino), "desconocido");

    // El primer commit: ninguno de los archivos que existían cambió.
    git(&paquete, &["add", "-A"]);
    git(&paquete, &["commit", "-qm", "uno"]);
    assert_eq!(commit_del_binario(&paquete, &destino), git(&paquete, &["rev-parse", "--short", "HEAD"]));

    // Tras `pack-refs` la rama vive solo en `packed-refs`; el commit siguiente
    // crea el archivo suelto y ni `HEAD` ni `packed-refs` cambian.
    git(&paquete, &["pack-refs", "--all"]);
    assert_eq!(commit_del_binario(&paquete, &destino), git(&paquete, &["rev-parse", "--short", "HEAD"]));
    for n in 0..2 {
        std::fs::write(paquete.join("cambio.txt"), format!("{n}")).unwrap();
        git(&paquete, &["add", "-A"]);
        git(&paquete, &["commit", "-qm", "otro"]);
        let esperado = git(&paquete, &["rev-parse", "--short", "HEAD"]);
        assert_eq!(commit_del_binario(&paquete, &destino), esperado, "commit {n} tras pack-refs");
    }
}
