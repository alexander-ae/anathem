use anathem::proyecto::{detectar, resolver, SIN_PROYECTO};
use std::fs;
use std::path::Path;

/// Crea un repo de mentira: un `.git/` con el config que se le pase.
fn repo(raiz: &Path, url_remoto: Option<&str>) {
    fs::create_dir_all(raiz.join(".git")).unwrap();
    let mut cfg = String::from("[core]\n\trepositoryformatversion = 0\n");
    if let Some(u) = url_remoto {
        cfg.push_str(&format!("[remote \"origin\"]\n\turl = {u}\n\tfetch = +refs/heads/*\n"));
    }
    fs::write(raiz.join(".git/config"), cfg).unwrap();
}

#[test]
fn usa_el_nombre_del_remoto_y_no_el_de_la_carpeta() {
    // El caso que motivó todo: el repo `api-monolito` vive en `API/Web`.
    let d = tempfile::tempdir().unwrap();
    let w = d.path().join("API").join("Web");
    fs::create_dir_all(&w).unwrap();
    repo(&w, Some("git@github.com:acme/api-monolito.git"));
    assert_eq!(detectar(&w).as_deref(), Some("api-monolito"));
}

#[test]
fn entiende_una_url_https() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("https://github.com/alexander-ae/anathem.git"));
    assert_eq!(detectar(d.path()).as_deref(), Some("anathem"));
}

#[test]
fn una_url_sin_sufijo_git_tambien_sirve() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("https://gitlab.com/grupo/proyecto"));
    assert_eq!(detectar(d.path()).as_deref(), Some("proyecto"));
}

#[test]
fn desde_un_subdirectorio_encuentra_la_raiz() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("git@host:g/repo.git"));
    let hondo = d.path().join("src").join("modulo").join("interno");
    fs::create_dir_all(&hondo).unwrap();
    assert_eq!(detectar(&hondo).as_deref(), Some("repo"));
}

#[test]
fn en_un_worktree_da_el_proyecto_y_no_el_nombre_de_la_rama() {
    // Este es el caso que la derivación ingenua rompe: dentro de
    // `.claude/worktrees/op200-implementation`, el basename es el de la rama.
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

    assert_eq!(detectar(&wt).as_deref(), Some("api-monolito"),
        "en un worktree tiene que dar el proyecto, no la rama");
}

#[test]
fn sin_remoto_cae_al_nombre_de_la_carpeta_del_repo() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("mi-proyecto-local");
    fs::create_dir_all(&r).unwrap();
    repo(&r, None);
    assert_eq!(detectar(&r).as_deref(), Some("mi-proyecto-local"));
}

#[test]
fn fuera_de_un_repo_no_detecta_nada() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(detectar(d.path()), None);
}

#[test]
fn un_project_explicito_le_gana_a_la_deteccion() {
    let d = tempfile::tempdir().unwrap();
    repo(d.path(), Some("git@host:g/detectado.git"));
    assert_eq!(resolver(Some("elegido".into()), d.path()), "elegido");
    assert_eq!(resolver(None, d.path()), "detectado");
}

#[test]
fn sin_repo_y_sin_pedido_queda_el_generico() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(resolver(None, d.path()), SIN_PROYECTO);
}
