use anathem::project::{detect, resolve, NO_PROJECT};
use std::fs;
use std::path::Path;

/// Creates a fake repo: a `.git/` with whatever config it is handed.
fn repo(raiz: &Path, url_remoto: Option<&str>) {
    fs::create_dir_all(raiz.join(".git")).unwrap();
    let mut cfg = String::from("[core]\n\trepositoryformatversion = 0\n");
    if let Some(u) = url_remoto {
        cfg.push_str(&format!("[remote \"origin\"]\n\turl = {u}\n\tfetch = +refs/heads/*\n"));
    }
    fs::write(raiz.join(".git/config"), cfg).unwrap();
}

#[test]
fn it_uses_the_remote_name_and_not_the_folder_name() {
    // The case that motivated all of this: the `api-monolito` repo lives in `API/Web`.
    let d = tempfile::tempdir().unwrap();
    let w = d.path().join("API").join("Web");
    fs::create_dir_all(&w).unwrap();
    repo(&w, Some("git@github.com:acme/api-monolito.git"));
    assert_eq!(detect(&w).as_deref(), Some("api-monolito"));
}

#[test]
fn it_understands_an_https_url() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("https://gitlab.com/__alexander__/anathem.git"));
    assert_eq!(detect(d.path()).as_deref(), Some("anathem"));
}

#[test]
fn a_url_without_the_git_suffix_works_too() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("https://gitlab.com/grupo/proyecto"));
    assert_eq!(detect(d.path()).as_deref(), Some("proyecto"));
}

#[test]
fn from_a_subdirectory_it_finds_the_root() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("git@host:g/repo.git"));
    let hondo = d.path().join("src").join("modulo").join("interno");
    fs::create_dir_all(&hondo).unwrap();
    assert_eq!(detect(&hondo).as_deref(), Some("repo"));
}

#[test]
fn in_a_worktree_it_gives_the_project_and_not_the_branch_name() {
    // This is the case naive derivation breaks: inside
    // `.claude/worktrees/op200-implementation`, the basename is the
    // branch's.
    let d = tempfile::tempdir().unwrap();
    let principal = d.path().join("Web");
    fs::create_dir_all(&principal).unwrap();
    repo(&principal, Some("git@github.com:acme/api-monolito.git"));

    let wt = principal.join(".claude").join("worktrees").join("op200-implementation-0c4c16");
    fs::create_dir_all(&wt).unwrap();
    let gitdir = principal.join(".git").join("worktrees").join("op200");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();

    assert_eq!(detect(&wt).as_deref(), Some("api-monolito"),
        "en un worktree tiene que dar el proyecto, no la rama");
}

#[test]
fn with_no_remote_it_falls_back_to_the_repo_folder_name() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mi-proyecto-local");
    fs::create_dir_all(&r).unwrap();
    repo(&r, None);
    assert_eq!(detect(&r).as_deref(), Some("mi-proyecto-local"));
}

#[test]
fn outside_a_repo_it_detects_nothing() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(detect(d.path()), None);
}

#[test]
fn an_explicit_project_beats_detection() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("git@host:g/detectado.git"));
    assert_eq!(resolve(Some("elegido".into()), d.path()), "elegido");
    assert_eq!(resolve(None, d.path()), "detectado");
}

#[test]
fn with_no_repo_and_nothing_asked_the_generic_one_stands() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(resolve(None, d.path()), NO_PROJECT);
}
