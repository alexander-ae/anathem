//! Which project a memory belongs to, when nobody says.
//!
//! Without this, everything saved without `--project` lands in
//! `sin-proyecto` and filtering by project stops being useful — which is
//! exactly the flaw of the previous system, where all 390 documents lived in
//! a single filler container.

use std::path::{Path, PathBuf};

pub const NO_PROJECT: &str = "sin-proyecto";

/// Derives the project name from the working directory.
///
/// It prefers the **remote's name** over the directory's, and that is not a
/// whim: a repository is usually cloned into a folder that is not named
/// after it — `api-monolito` lives in `API/Web` — and inside a worktree the
/// directory is named after the branch, not the project. The remote is the
/// only name stable across clones, worktrees and machines.
///
/// It does not invoke `git`: it reads `.git` directly. That way it works
/// even when git is not installed, costs no subprocess, and does not depend
/// on the binary inheriting a sane PATH.
pub fn detect(cwd: &Path) -> Option<String> {
    let comun = common_git_dir(cwd)?;
    if let Some(n) = remote_name(&comun.join("config")) {
        return Some(n);
    }
    // With no remote, the name of the folder holding the common `.git`.
    // That is the main repository even when standing in a worktree.
    comun.parent()?.file_name()?.to_str().map(str::to_string)
}

/// The **common** `.git` directory: the main repository's, not the
/// worktree's. In a worktree, `.git` is a file pointing at
/// `<main>/.git/worktrees/<name>`, and inside there is a `commondir` file
/// with the relative path back to the real `.git`.
fn common_git_dir(cwd: &Path) -> Option<PathBuf> {
    let mut d = cwd;
    loop {
        let punto_git = d.join(".git");
        if punto_git.is_dir() {
            return Some(punto_git);
        }
        if punto_git.is_file() {
            let content = std::fs::read_to_string(&punto_git).ok()?;
            let pack_path = content.strip_prefix("gitdir:")?.trim();
            let gitdir = if Path::new(pack_path).is_absolute() {
                PathBuf::from(pack_path)
            } else {
                d.join(pack_path)
            };
            let commondir = gitdir.join("commondir");
            if let Ok(rel) = std::fs::read_to_string(&commondir) {
                let rel = rel.trim();
                let c = if Path::new(rel).is_absolute() {
                    PathBuf::from(rel)
                } else {
                    gitdir.join(rel)
                };
                return normalize(&c);
            }
            return normalize(&gitdir);
        }
        d = d.parent()?;
    }
}

/// Resolves a path's `..` without touching disk. `commondir` is usually
/// `../..`, and leaving it unresolved would make `parent()` return the wrong
/// component when looking for the folder's name.
fn normalize(p: &Path) -> Option<PathBuf> {
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

/// Extracts the repository name from `origin`'s url in a git config. It
/// supports the two usual shapes: `git@host:group/repo.git` and
/// `https://host/group/repo.git`.
fn remote_name(config: &Path) -> Option<String> {
    let text = std::fs::read_to_string(config).ok()?;
    let mut en_origin = false;
    for line_ in text.lines() {
        let l = line_.trim();
        if l.starts_with('[') {
            en_origin = l.replace(char::is_whitespace, "") == "[remote\"origin\"]";
            continue;
        }
        if !en_origin {
            continue;
        }
        if let Some(url) = l.strip_prefix("url") {
            let url = url.trim_start().strip_prefix('=')?.trim();
            let last = url.rsplit(['/', ':']).next()?;
            let name = last.strip_suffix(".git").unwrap_or(last);
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// The project to use: the caller's, or the detected one, or the generic
/// fallback. An explicit `--project` always wins.
pub fn resolve(requested: Option<String>, cwd: &Path) -> String {
    requested.unwrap_or_else(|| detect(cwd).unwrap_or_else(|| NO_PROJECT.to_string()))
}
