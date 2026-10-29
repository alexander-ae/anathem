//! `build.rs` has to keep the commit current: if it freezes on the first
//! build's, `mem --version` reports a commit that is no longer the binary's —
//! exactly the failure it exists to make visible.
//!
//! It builds a minimal package with a copy of the real `build.rs` inside a
//! temporary repository, and rebuilds it after each change of commit.

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

/// Builds and runs the minimal package; returns the commit it believes it has.
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

    // With no commit at all there is nothing to report, and it cannot fail.
    assert_eq!(binary_commit(&pack_, &target), "desconocido");

    // The first commit: none of the files that existed changed.
    git(&pack_, &["add", "-A"]);
    git(&pack_, &["commit", "-qm", "uno"]);
    assert_eq!(binary_commit(&pack_, &target), git(&pack_, &["rev-parse", "--short", "HEAD"]));

    // After `pack-refs` the branch lives only in `packed-refs`; the next
    // commit creates the loose file and neither `HEAD` nor `packed-refs`
    // changes.
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
