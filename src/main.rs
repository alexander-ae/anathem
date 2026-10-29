use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "mem",
    about = "anathem — memoria local para agentes",
    version = anathem::doctor::FULL_VERSION
)]
struct Cli {
    /// Ruta de la base de datos
    #[arg(long, global = true, default_value = "~/.anathem/mem.db")]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Crea la base de datos si no existe y muestra su ruta
    Init,
    /// Importa el export congelado de supermemory
    Migrate {
        /// Ruta del JSONL exportado
        #[arg(long, default_value = "data/supermemory-export.jsonl")]
        from: PathBuf,
    },
    /// Busca en la memoria
    Search {
        /// Texto de la consulta
        query: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Muestra el score bm25 crudo junto a cada resultado
        #[arg(long)]
        explain: bool,
        /// Tope de tokens que puede ocupar la respuesta. Por defecto 1200,
        /// que es lo que se inyecta en el contexto de un agente. Subirlo
        /// sirve para inspeccionar más candidatos de los que entrarían.
        #[arg(long)]
        budget: Option<usize>,
    },
    /// Guarda una memoria nueva
    Save {
        /// Cuerpo de la memoria
        text: String,
        /// Proyecto al que pertenece. Sin esto se deriva del repositorio
        /// donde estés parado: el nombre del remoto `origin`, o el de la
        /// carpeta del repo si no hay remoto.
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        /// Clave de tema. Si ya hay una memoria activa con esta clave en el
        /// proyecto, la nueva la reemplaza (queda superseded, no se borra ni
        /// se edita). Usala cuando lo que guardás es una versión actualizada
        /// de algo ya guardado. Mirá las existentes con `mem topics`.
        #[arg(long)]
        topic: Option<String>,
    },
    /// Edita el cuerpo de una memoria existente en $EDITOR
    Edit {
        /// id de la memoria a editar
        id: i64,
    },
    /// Exporta la memoria a archivos markdown con frontmatter
    Export {
        #[arg(long, default_value = "export")]
        out: PathBuf,
    },
    /// Reconstruye chunks y el índice FTS5 desde memories.body
    Reindex,
    /// Corre el harness de evaluación
    Eval {
        /// Por defecto, `evalset.json` junto a la base de datos. Relativo al
        /// directorio actual sería frágil: correr el mismo comando desde otra
        /// carpeta cambiaría en silencio contra qué se mide.
        #[arg(long)]
        evalset: Option<PathBuf>,
        #[arg(long, default_value = "anathem bm25")]
        label: String,
        /// Escribe el JSON de resultados en este archivo
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Arranca el servidor MCP sobre stdio
    Serve,
    /// Marca una memoria como reemplazada por otra: la vieja deja de salir en búsquedas
    Supersede {
        /// id de la memoria nueva que reemplaza a la vieja
        newer: i64,
        /// id de la memoria vieja que queda superseded
        older: i64,
    },
    /// Fija una memoria para que sobreviva el recorte de candidatos en la búsqueda
    Pin {
        id: i64,
    },
    /// Quita el pin de una memoria
    Unpin {
        id: i64,
    },
    /// Saca una memoria de la búsqueda sin borrarla
    Forget {
        id: i64,
    },
    /// Lista las claves de tema: proyecto, cuántas memorias tuvo cada una y cuál es la vigente
    Topics,
    /// Qué se guardó últimamente, de más nuevo a más viejo, y quién lo guardó
    ///
    /// La búsqueda ordena por relevancia; esto ordena por fecha. Sirve para
    /// retomar un trabajo, o para ver qué dejó otro agente antes de seguirlo.
    Recent {
        /// Proyecto; por defecto el del repositorio donde estás parado
        #[arg(long)]
        project: Option<String>,
        /// Todos los proyectos, no solo uno
        #[arg(long = "todos", conflicts_with = "project")]
        all_projects: bool,
        /// Ventana hacia atrás: 6h, 90m, 2d. Por defecto 24h; `--desde todo` no acota
        #[arg(long = "desde", value_name = "DESDE", default_value = "24h")]
        since: String,
        /// Cuántas memorias como máximo
        #[arg(long = "limite", value_name = "LIMITE", default_value_t = 15)]
        limit: usize,
    },
    /// Muestra la cadena de supersession de una memoria, de la más nueva a la más vieja
    History {
        id: i64,
    },
    /// Importa lo que escribió `mem export`: restaurar, mudar de máquina o
    /// sincronizar dos computadoras a mano
    Import {
        /// Carpeta que escribió `mem export --out`
        #[arg(long = "desde", value_name = "DESDE")]
        from: PathBuf,
        /// Importar sobre una base que ya tiene memorias, agregando solo lo
        /// que falte
        #[arg(long = "fusionar")]
        merge: bool,
    },
    /// Comprueba si hay una versión más nueva publicada
    ///
    /// Consulta la red solo cuando lo pedís: anathem no habla con ningún
    /// servidor por su cuenta. No baja ni reemplaza nada; te dice qué correr.
    Update,
    /// Diagnostica la instalación sin modificar nada
    ///
    /// Sale con 0 si no hay problemas (los avisos y la información no
    /// cuentan), con 1 si los hay y con 2 si no pudo correr.
    Doctor,
    /// Administra el paquete de vocabulario de la capa vectorial
    Model {
        #[command(subcommand)]
        action: ModelCmd,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    /// Instala un paquete desde un archivo local y reindexa
    Install {
        /// Ruta del archivo .anathem-pack
        #[arg(value_name = "ARCHIVO")]
        file: PathBuf,
    },
    /// Muestra el paquete activo y la cobertura de vectores
    Status,
    /// Quita el paquete y sus vectores; la búsqueda vuelve a ser solo bm25
    Remove,
}

/// Resolves a `~/` prefix against `home`, without touching the environment —
/// which makes it testable without depending on global process variables.
fn resolve(p: &str, home: Option<&Path>) -> Result<PathBuf> {
    match p.strip_prefix("~/") {
        Some(rest) => {
            let home = home
                .ok_or_else(|| anyhow::anyhow!("no está definida ni HOME ni USERPROFILE"))?;
            Ok(home.join(rest))
        }
        None => Ok(PathBuf::from(p)),
    }
}

/// Picks which of the two variables acts as the user's home.
///
/// Precedence depends on the platform, and runs the opposite way to what
/// seems intuitive: on Windows `USERPROFILE` wins, not `HOME`. Git Bash and
/// MSYS define `HOME` on Windows too, but in posix form
/// (`/c/Users/someone`); for a native Windows binary that is not a valid
/// user path — a leading `/` with no drive letter resolves against the root
/// of the current drive, not against the home — and the same person would
/// run `mem` from `cmd.exe` and from Git Bash against two different
/// databases, with no warning. Outside Windows `USERPROFILE` never exists,
/// so there `HOME` always wins and this branch changes nothing.
///
/// A variable that is defined but **empty** counts as undefined: otherwise
/// `resolve()` would join it to the rest of the path all the same and
/// produce a path relative to the current directory, with no error and no
/// warning.
///
/// It takes the values already read, and whether to treat this as the
/// Windows case, as a boolean, rather than reading them itself or asking
/// the platform — `dirs_home` does that — so the tests can exercise both
/// precedences without depending on which system they run on.
fn choose_home(home: Option<PathBuf>, userprofile: Option<PathBuf>, is_windows: bool) -> Option<PathBuf> {
    let no_vacia = |p: Option<PathBuf>| p.filter(|p| !p.as_os_str().is_empty());
    let home = no_vacia(home);
    let userprofile = no_vacia(userprofile);
    if is_windows {
        userprofile.or(home)
    } else {
        home.or(userprofile)
    }
}

fn dirs_home() -> Option<PathBuf> {
    choose_home(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        cfg!(windows),
    )
}

/// The editor `mem edit` uses when `$EDITOR` is not defined.
///
/// `vi` does not exist in a Windows installation: with no `$EDITOR`,
/// spawning the process would fail outright. `notepad` is native there. It
/// takes the boolean rather than asking the platform so both branches can
/// be tested on any system.
fn default_editor(is_windows: bool) -> &'static str {
    if is_windows {
        "notepad"
    } else {
        "vi"
    }
}

/// Fills in whatever vectors are missing after a write path.
///
/// The four paths that write (migrate, save, edit, reindex) call this and
/// not `vecstore::sync` directly, so they cannot drift apart: `Cmd::Edit`
/// used to be the one that forgot, and an edited memory went without
/// vectors until the next `mem reindex`.
///
/// Trouble in the vector layer is reported, it does not bring the command
/// down: the memory is already saved, and losing the warning would be worse
/// than losing the vector.
fn sync_after_write(conn: &rusqlite::Connection, dir: &Path) {
    use anathem::vecstore::SyncOutcome;
    let active = anathem::packstore::resolve(conn, dir);
    active.warn();
    let Some((p, identity)) = active.for_writing() else {
        return;
    };
    match anathem::vecstore::sync(conn, p, identity) {
        Ok(SyncOutcome::Complete(0)) => {}
        Ok(SyncOutcome::Complete(n)) => println!("vectores nuevos: {n}"),
        // Another process installed or removed the pack while this one was
        // embedding. What was written before the change its `adopt_pack`
        // already deleted, and what was left was never written: some chunks
        // end up without a vector, which is the acceptable degradation.
        // Mixing them would not be. The command still exits 0: the memory
        // is saved.
        Ok(SyncOutcome::Abandoned(_)) => eprintln!(
            "aviso: la memoria se guardó, pero otro proceso cambió el paquete de \
             vocabulario mientras se calculaban los vectores; se abandonó el lote para \
             no mezclar dos paquetes. Corré `mem reindex` cuando puedas."
        ),
        Err(e) => eprintln!(
            "aviso: la memoria se guardó pero no se pudieron actualizar los vectores ({e}); \
             corré `mem reindex` cuando puedas."
        ),
    }
}

fn main() {
    let cli = Cli::parse();
    let is_doctor = matches!(cli.cmd, Cmd::Doctor);
    if let Err(e) = run(cli) {
        // The same format `fn main() -> Result<()>` would print. `doctor`
        // exiting 2 and not 1 is what lets a script tell "found problems"
        // (1) from "could not run" (2); every other command keeps the usual
        // 1.
        eprintln!("Error: {e:?}");
        std::process::exit(if is_doctor { anathem::doctor::EXIT_BROKEN } else { 1 });
    }
}

fn run(cli: Cli) -> Result<()> {
    let path = resolve(&cli.db, dirs_home().as_deref())?;
    // If there is a `tags.json` next to the database, its rules replace the
    // default taxonomy in `tags::classify` outright. It has to be set before
    // any operation that classifies memories (migrate, save, serve).
    anathem::tags::init(&path);
    match cli.cmd {
        Cmd::Init => {
            anathem::db::open(&path)?;
            println!("base lista en {}", path.display());
        }
        Cmd::Migrate { from } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let rep = anathem::migrate::run(&conn, &from)?;
            println!(
                "migradas={} archivadas={} fallidas={}",
                rep.migrated, rep.archived, rep.failed
            );
            if !rep.curated_failed.is_empty() {
                println!(
                    "curadas en status failed (no migradas, reimportar a mano):"
                );
                for (ext_id, title) in &rep.curated_failed {
                    println!("  {ext_id}  {title}");
                }
            }
            sync_after_write(&conn, &dir);
        }
        Cmd::Search { query, project, tag, limit, explain, budget } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            let p = anathem::packstore::resolve_and_warn(&conn, dir);
            let ctx = anathem::search::SearchContext::for_reading(p.as_ref(), cfg.bm25_weight);
            let q = anathem::search::Query {
                text: query, project, tags: tag, limit,
                budget_tokens: budget.unwrap_or(anathem::search::DEFAULT_BUDGET_TOKENS),
                // The CLI does not trim: `mem search` still shows the whole
                // chunk, as it always has.
                excerpt: None,
            };
            let hits = anathem::search::run_explained(&conn, &q, &ctx)?;
            if hits.is_empty() {
                // Without this an empty search prints nothing and the user
                // cannot tell "there is nothing" from "the binary is broken".
                let total: i64 = conn.query_row(
                    "select count(*) from memories where status = 'active'", [], |r| r.get(0))?;
                if total == 0 {
                    println!("Sin resultados: la memoria está vacía. Guardá algo con `mem save \"...\"`.");
                } else {
                    let plural = if total == 1 { "memoria activa" } else { "memorias activas" };
                    println!("Sin resultados entre {total} {plural}.");
                    if p.is_some() {
                        // With the vector layer active, search is no longer
                        // purely lexical: the old advice ("use the exact
                        // nouns") would stop being true and would confuse
                        // more than it helps.
                        println!("La búsqueda combina bm25 con similitud vectorial. Probá con otros términos o revisá `mem model status` si esperabas más cobertura.");
                    } else {
                        println!("La búsqueda es léxica: probá con sustantivos que estén escritos en la memoria, no con la forma en que lo preguntarías en voz alta.");
                    }
                }
                return Ok(());
            }
            for (i, e) in hits.iter().enumerate() {
                if explain {
                    // A result may have come in through bm25, through the
                    // vector, or through both: we show every signal there is,
                    // so it is clear which retriever brought it in, not only
                    // the one that won.
                    let señales = match (e.bm25, e.cosine) {
                        (Some(b), Some(c)) => format!("bm25={b:.3} coseno={c:.3}"),
                        (Some(b), None) => format!("bm25={b:.3}"),
                        (None, Some(c)) => format!("coseno={c:.3}, solo vectorial"),
                        (None, None) => "sin señal".to_string(),
                    };
                    println!("{:2}. [{}] {}  ({señales})", i + 1, e.hit.project, e.hit.title);
                } else {
                    println!("{:2}. [{}] {}", i + 1, e.hit.project, e.hit.title);
                }
                println!("    {}", e.hit.snippet.chars().take(180).collect::<String>());
            }
        }
        Cmd::Save { text, project, tag, topic } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            // When the project is derived instead of given, say so: an
            // implicit value nobody sees is the one nobody can later explain
            // the wrongness of.
            let explicito = project.is_some();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let project = anathem::project::resolve(project, &cwd);
            let g = anathem::ingest::save_with_topic(&conn, &text, &project, tag, topic.as_deref(), None)?;
            let labels = if g.tags.is_empty() { String::new() } else { format!(" [{}]", g.tags.join(", ")) };
            let como = if explicito { "" } else { " (detectado)" };
            println!("guardada memoria {} en {project}{como}{labels}", g.id);
            if let Some(msg) = g.describe() {
                println!("{msg}");
            }
            sync_after_write(&conn, &dir);
        }
        Cmd::Edit { id } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let body: String = conn.query_row(
                "select body from memories where id = ?1",
                [id],
                |r| r.get(0),
            )?;
            let tmp = std::env::temp_dir().join(format!("anathem-edit-{id}.md"));
            std::fs::write(&tmp, &body)?;
            let editor = std::env::var("EDITOR")
                .unwrap_or_else(|_| default_editor(cfg!(windows)).to_string());
            let status = std::process::Command::new(&editor).arg(&tmp).status()?;
            if !status.success() {
                anyhow::bail!("{editor} salió con error, no se guardó nada");
            }
            let new_id = std::fs::read_to_string(&tmp)?;
            std::fs::remove_file(&tmp).ok();
            if new_id != body {
                // `replace_body` re-chunks, and the cascade takes the old
                // text's vectors with it. Without this, nobody rebuilt the
                // new ones: the edited memory stayed invisible to vector
                // search until the next `mem reindex`.
                anathem::edit::replace_body(&conn, id, &new_id)?;
                println!("memoria {id} actualizada");
                sync_after_write(&conn, &dir);
            } else {
                println!("sin cambios");
            }
        }
        Cmd::Export { out } => {
            let conn = anathem::db::open(&path)?;
            let n = anathem::export::to_markdown(&conn, &out)?;
            println!("exportadas {n} memorias a {}", out.display());
        }
        Cmd::Reindex => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let n = anathem::edit::reindex(&conn)?;
            println!("reindexadas {n} memorias");
            sync_after_write(&conn, &dir);
        }
        Cmd::Eval { evalset, label, out } => {
            let evalset = evalset.unwrap_or_else(|| {
                path.parent().unwrap_or(std::path::Path::new(".")).join("evalset.json")
            });
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            let p = anathem::packstore::resolve_and_warn(&conn, dir);
            let ctx = anathem::search::SearchContext::for_reading(p.as_ref(), cfg.bm25_weight);
            let r = anathem::evalrun::run(&conn, &evalset, &label, &ctx)?;
            println!(
                "{}  recall@5={:.0}%  recall@50={:.0}%  MRR={:.3}  p50={:.1}ms  n={}",
                r.system, r.recall_at_5 * 100.0, r.recall_at_50 * 100.0,
                r.mrr, r.p50_ms, r.n_cases
            );
            if let Some(o) = out {
                if let Some(p) = o.parent() { std::fs::create_dir_all(p)?; }
                std::fs::write(&o, serde_json::to_string_pretty(&r)?)?;
                println!("-> {}", o.display());
            }
        }
        Cmd::Serve => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            anathem::mcp::serve(&conn, dir, &cfg)?;
        }
        Cmd::Supersede { newer, older } => {
            let conn = anathem::db::open(&path)?;
            anathem::supersede::link(&conn, newer, older)?;
            println!("memoria {older} marcada como superseded por {newer}");
        }
        Cmd::Pin { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::pin(&conn, id, true)?;
            println!("memoria {id} fijada (pinned)");
        }
        Cmd::Unpin { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::pin(&conn, id, false)?;
            println!("memoria {id} ya no está fijada");
        }
        Cmd::Forget { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::forget(&conn, id)?;
            println!("memoria {id} olvidada (sigue en la base, fuera de la búsqueda)");
        }
        Cmd::Topics => {
            let conn = anathem::db::open(&path)?;
            let topics = anathem::maint::topics(&conn)?;
            if topics.is_empty() {
                println!("Sin claves de tema. Se crean guardando con `mem save \"...\" --topic <clave>`.");
            } else {
                let ancho_p = topics.iter().map(|t| t.project.chars().count()).max().unwrap_or(0).max("proyecto".len());
                let ancho_t = topics.iter().map(|t| t.topic.chars().count()).max().unwrap_or(0).max("tema".len());
                println!("{:<ancho_p$}  {:<ancho_t$}  {:>8}  vigente", "proyecto", "tema", "memorias");
                for t in topics {
                    let current_one = t.current_one.map_or("ninguna".to_string(), |v| v.to_string());
                    println!("{:<ancho_p$}  {:<ancho_t$}  {:>8}  {current_one}", t.project, t.topic, t.total);
                }
            }
        }
        Cmd::Import { from, merge } => {
            let conn = anathem::db::open(&path)?;
            let r = anathem::import::import_dir(&conn, &from, merge)?;
            println!(
                "importadas {} memorias, {} omitidas por estar ya, {} cadenas de supersession rearmadas",
                r.inserted, r.skipped, r.chains
            );
            if r.inserted > 0 {
                println!("Si tenés paquete de vocabulario, corré `mem reindex` para los vectores.");
            }
        }
        Cmd::Update => {
            anathem::update::check_for_update()?;
        }
        Cmd::Recent { project, all_projects, since, limit } => {
            let conn = anathem::db::open(&path)?;
            let window = if since.trim().eq_ignore_ascii_case("todo") {
                None
            } else {
                Some(anathem::recent::duration_in_seconds(&since)?)
            };
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project_ = if all_projects {
                None
            } else {
                Some(anathem::project::resolve(project, &cwd))
            };
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            let items = anathem::recent::list_recent(
                &conn, project_.as_deref(), window, limit, cfg.excerpt)?;
            println!("{}", anathem::recent::format_items(&items, project_.is_none()));
        }
        Cmd::History { id } => {
            let conn = anathem::db::open(&path)?;
            let cadena = anathem::maint::history(&conn, id)?;
            for (i, title, status) in cadena {
                println!("{i:5}  [{status:10}]  {title}");
            }
        }
        Cmd::Doctor => {
            // Not `db::open`: that one opens for writing and creates
            // whatever is missing, and a diagnostic cannot modify what it is
            // diagnosing.
            let informe = anathem::doctor::diagnose(&path);
            print!("{}", informe.render());
            std::io::Write::flush(&mut std::io::stdout())?;
            if informe.problems() > 0 {
                std::process::exit(anathem::doctor::EXIT_PROBLEMS);
            }
        }
        Cmd::Model { action } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            match action {
                ModelCmd::Install { file } => {
                    let i = anathem::packstore::install(&conn, &dir, &file)?;
                    println!(
                        "paquete {} instalado ({} dimensiones), {} fragmentos indexados",
                        i.name, i.dims, i.chunks
                    );
                    if i.displaced {
                        // The file was installed, but another process took
                        // over the registry while this one was indexing and
                        // the index was left half done. Say it: it is the
                        // only thing separating "installed and working" from
                        // "installed, and someone else is in charge".
                        eprintln!(
                            "aviso: otro proceso cambió el paquete de vocabulario mientras se \
                             indexaba; esta instalación quedó desplazada y el índice a medias. \
                             Mirá `mem model status` y reinstalá el que quieras que quede."
                        );
                    }
                }
                ModelCmd::Status => match anathem::vecstore::registered_pack(&conn)? {
                    None => println!("sin paquete: la búsqueda usa solo bm25"),
                    Some((name, sha, dims)) => {
                        let (con, tot) = anathem::vecstore::coverage(&conn, dims)?;
                        let pack_path = anathem::packstore::pack_path(&dir, &name);
                        println!("paquete {name} ({dims} dimensiones, sha {})", &sha[..16.min(sha.len())]);
                        println!("cobertura: {con}/{tot} fragmentos");
                        if !pack_path.exists() {
                            println!("FALTA el archivo {}: la búsqueda está degradada a bm25", pack_path.display());
                        } else if con < tot {
                            println!("hay {} fragmentos sin vector usable; corré `mem reindex`", tot - con);
                        }
                        // `status` reads `meta` directly, without going
                        // through `resolve`, so it is the only command that
                        // can see the mark without clearing it. Worth
                        // saying: while it is set, what the lines above
                        // report describes the pack from before the
                        // interrupted install, not the file on disk.
                        if anathem::vecstore::pending_install(&conn)?.is_some() {
                            println!(
                                "hay una instalación de {name} a medias: el archivo en disco \
                                 puede no ser este. La próxima búsqueda reindexa."
                            );
                        }
                    }
                },
                ModelCmd::Remove => match anathem::packstore::remove_pack(&conn, &dir)? {
                    Some(name) => {
                        println!("paquete {name} quitado; la búsqueda vuelve a ser solo bm25")
                    }
                    None => println!("no había ningún paquete instalado"),
                },
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_without_home_returns_err_instead_of_panicking() {
        let err = resolve("~/.anathem/mem.db", None)
            .expect_err("sin HOME, resolve debe fallar en vez de panicar");
        assert!(err.to_string().contains("HOME"), "mensaje inesperado: {err}");
    }

    #[test]
    fn resolve_with_home_expands_the_prefix() {
        let home = Path::new("/home/alguien");
        let resuelto = resolve("~/.anathem/mem.db", Some(home)).unwrap();
        assert_eq!(resuelto, home.join(".anathem/mem.db"));
    }

    #[test]
    fn resolve_without_prefix_leaves_the_path_alone() {
        let resuelto = resolve("/tmp/x.db", None).unwrap();
        assert_eq!(resuelto, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn choose_home_outside_windows_prefers_home_over_userprofile() {
        let h = choose_home(
            Some(PathBuf::from("/home/alguien")),
            Some(PathBuf::from(r"C:\Users\alguien")),
            false,
        );
        assert_eq!(h, Some(PathBuf::from("/home/alguien")));
    }

    #[test]
    fn choose_home_on_windows_prefers_userprofile_over_home() {
        // Git Bash and MSYS define HOME on Windows too, but in posix form
        // (`/c/Users/...`), which a native binary does not read as a user
        // path. USERPROFILE is always the native one: it has to win, even
        // when both are defined.
        let h = choose_home(
            Some(PathBuf::from("/c/Users/alguien")),
            Some(PathBuf::from(r"C:\Users\alguien")),
            true,
        );
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn choose_home_uses_userprofile_when_there_is_no_home() {
        // The Windows-without-Git-Bash case: HOME is not defined and we
        // have to fall back to USERPROFILE instead of failing.
        let h = choose_home(None, Some(PathBuf::from(r"C:\Users\alguien")), false);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn choose_home_on_windows_falls_back_to_home_without_userprofile() {
        // USERPROFILE wins when both are there, but if it alone is missing
        // there is still a usable home in HOME.
        let h = choose_home(Some(PathBuf::from(r"C:\Users\alguien")), None, true);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn choose_home_an_empty_home_does_not_count_as_defined() {
        // HOME can exist as a variable and be empty (export HOME=).
        // Without this check, `resolve` would join the rest of the path
        // against "" and the result would be relative to the current
        // directory, silently.
        let h = choose_home(Some(PathBuf::new()), Some(PathBuf::from(r"C:\Users\alguien")), false);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn choose_home_an_empty_userprofile_does_not_count_as_defined() {
        let h = choose_home(Some(PathBuf::from("/home/alguien")), Some(PathBuf::new()), true);
        assert_eq!(h, Some(PathBuf::from("/home/alguien")));
    }

    #[test]
    fn choose_home_with_neither_of_the_two_returns_none() {
        assert_eq!(choose_home(None, None, false), None);
        assert_eq!(choose_home(None, None, true), None);
    }

    #[test]
    fn default_editor_on_windows_is_notepad() {
        assert_eq!(default_editor(true), "notepad");
    }

    #[test]
    fn default_editor_outside_windows_is_vi() {
        assert_eq!(default_editor(false), "vi");
    }

    #[test]
    fn resolve_without_home_names_both_variables() {
        // Stronger than the original assertion: it is not enough for the
        // message to mention HOME, it has to mention the Windows
        // alternative too, because the fallback now exists and whoever
        // reads the error on that platform needs to know USERPROFILE is the
        // one to define.
        let err = resolve("~/.anathem/mem.db", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HOME"), "mensaje inesperado: {msg}");
        assert!(msg.contains("USERPROFILE"), "mensaje inesperado: {msg}");
    }
}
