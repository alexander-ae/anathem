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
fn binary_commit(dir: &Path, target: &Path) -> String {
    let o = Command::new(env!("CARGO"))
        .args(["run", "-q", "--offline"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn has_git() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn the_binary_commit_tracks_head_after_pack_refs_and_on_the_first_commit() {
    if !has_git() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let pack_ = d.path().join("mini");
    let target = d.path().join("target");
    std::fs::create_dir_all(pack_.join("src")).unwrap();
    std::fs::write(
        pack_.join("Cargo.toml"),
        "[package]\nname = \"mini\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(pack_.join("build.rs"), include_str!("../build.rs")).unwrap();
    std::fs::write(
        pack_.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"ANATHEM_COMMIT\")); }\n",
    )
    .unwrap();
    std::fs::write(pack_.join(".gitignore"), "target\n").unwrap();
    git(&pack_, &["init", "-q"]);

    // Sin ningún commit no hay nada que decir, y no puede fallar.
    assert_eq!(binary_commit(&pack_, &target), "desconocido");

    // El primer commit: ninguno de los archivos que existían cambió.
    git(&pack_, &["add", "-A"]);
    git(&pack_, &["commit", "-qm", "uno"]);
    assert_eq!(binary_commit(&pack_, &target), git(&pack_, &["rev-parse", "--short", "HEAD"]));

    // Tras `pack-refs` la rama vive solo en `packed-refs`; el commit siguiente
    // crea el archivo suelto y ni `HEAD` ni `packed-refs` cambian.
    git(&pack_, &["pack-refs", "--all"]);
    assert_eq!(binary_commit(&pack_, &target), git(&pack_, &["rev-parse", "--short", "HEAD"]));
    for n in 0..2 {
        std::fs::write(pack_.join("cambio.txt"), format!("{n}")).unwrap();
        git(&pack_, &["add", "-A"]);
        git(&pack_, &["commit", "-qm", "otro"]);
        let expected = git(&pack_, &["rev-parse", "--short", "HEAD"]);
        assert_eq!(binary_commit(&pack_, &target), expected, "commit {n} tras pack-refs");
    }
}
