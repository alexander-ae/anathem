use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "mem",
    about = "anathem — memoria local para agentes",
    version = anathem::doctor::VERSION_COMPLETA
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
        nueva: i64,
        /// id de la memoria vieja que queda superseded
        vieja: i64,
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
    /// Muestra la cadena de supersession de una memoria, de la más nueva a la más vieja
    History {
        id: i64,
    },
    /// Diagnostica la instalación sin modificar nada
    ///
    /// Sale con 0 si no hay problemas (los avisos y la información no
    /// cuentan), con 1 si los hay y con 2 si no pudo correr.
    Doctor,
    /// Administra el paquete de vocabulario de la capa vectorial
    Model {
        #[command(subcommand)]
        accion: ModelCmd,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    /// Instala un paquete desde un archivo local y reindexa
    Install {
        /// Ruta del archivo .anathem-pack
        archivo: PathBuf,
    },
    /// Muestra el paquete activo y la cobertura de vectores
    Status,
    /// Quita el paquete y sus vectores; la búsqueda vuelve a ser solo bm25
    Remove,
}

/// Resuelve un prefijo `~/` contra `home`, sin tocar el entorno — así es testeable
/// sin depender de variables de proceso globales.
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

/// Elige cuál de las dos variables hace de home del usuario.
///
/// La precedencia depende de la plataforma, y va al revés de lo que parece
/// intuitivo: en Windows gana `USERPROFILE`, no `HOME`. Git Bash y MSYS
/// definen `HOME` también en Windows, pero en formato posix
/// (`/c/Users/alguien`); para un binario nativo de Windows eso no es una
/// ruta de usuario válida — un `/` inicial sin letra de unidad se resuelve
/// contra la raíz de la unidad actual, no contra el home— y la misma
/// persona correría `mem` desde `cmd.exe` y desde Git Bash con dos bases
/// distintas y ningún aviso. Fuera de Windows, `USERPROFILE` no existe
/// nunca, así que ahí siempre gana `HOME` y esta rama no cambia nada.
///
/// Una variable definida pero **vacía** cuenta como no definida: de otro
/// modo `resolve()` la uniría igual al resto de la ruta y produciría una
/// ruta relativa al directorio actual, sin ningún error ni aviso.
///
/// Recibe los valores ya leídos y si hay que tratar el caso Windows como
/// un booleano, en vez de leerlos ella misma o preguntarle a la plataforma
/// —eso lo hace `dirs_home`—, para que los tests puedan ejercitar las dos
/// precedencias sin depender de en qué sistema corren.
fn elegir_home(home: Option<PathBuf>, userprofile: Option<PathBuf>, es_windows: bool) -> Option<PathBuf> {
    let no_vacia = |p: Option<PathBuf>| p.filter(|p| !p.as_os_str().is_empty());
    let home = no_vacia(home);
    let userprofile = no_vacia(userprofile);
    if es_windows {
        userprofile.or(home)
    } else {
        home.or(userprofile)
    }
}

fn dirs_home() -> Option<PathBuf> {
    elegir_home(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        cfg!(windows),
    )
}

/// El editor a usar para `mem edit` cuando `$EDITOR` no está definida.
///
/// `vi` no existe en una instalación de Windows: sin `$EDITOR`, lanzar el
/// proceso fallaría directamente. `notepad` sí es nativo ahí. Recibe el
/// booleano en vez de preguntarle a la plataforma para poder probar las
/// dos ramas en cualquier sistema.
fn editor_por_defecto(es_windows: bool) -> &'static str {
    if es_windows {
        "notepad"
    } else {
        "vi"
    }
}

/// Rellena los vectores que falten después de un camino de escritura.
///
/// Los cuatro caminos que escriben (migrar, guardar, editar, reindexar)
/// llaman a esto y no a `vecstore::sincronizar` directo, para que no puedan
/// divergir entre sí: antes `Cmd::Edit` era el único que se lo olvidaba, y
/// una memoria editada quedaba sin vectores hasta el próximo `mem reindex`.
///
/// Un problema de la capa vectorial se avisa, no tumba el comando: la
/// memoria ya está guardada y perder el aviso sería peor que perder el
/// vector.
fn sincronizar_tras_escribir(conn: &rusqlite::Connection, dir: &Path) {
    use anathem::vecstore::Sincronizacion;
    let activo = anathem::paquete::resolver(conn, dir);
    activo.avisar();
    let Some((p, identidad)) = activo.para_escribir() else {
        return;
    };
    match anathem::vecstore::sincronizar(conn, p, identidad) {
        Ok(Sincronizacion::Completa(0)) => {}
        Ok(Sincronizacion::Completa(n)) => println!("vectores nuevos: {n}"),
        // Otro proceso instaló o quitó el paquete mientras este embebía. Lo
        // que se escribió antes del cambio ya lo borró su `adoptar_pack`, y
        // lo que faltaba no se escribió: quedan fragmentos sin vector, que
        // es la degradación aceptable. Mezclarlos no lo sería. El comando
        // sale con 0 igual: la memoria está guardada.
        Ok(Sincronizacion::Abandonada(_)) => eprintln!(
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
    let es_doctor = matches!(cli.cmd, Cmd::Doctor);
    if let Err(e) = ejecutar(cli) {
        // Mismo formato que imprimiría `fn main() -> Result<()>`. Que `doctor`
        // salga con 2 y no con 1 es lo que permite a un script distinguir
        // "encontró problemas" (1) de "no pudo correr" (2); el resto de los
        // comandos conservan el 1 de siempre.
        eprintln!("Error: {e:?}");
        std::process::exit(if es_doctor { anathem::doctor::SALIDA_ROTO } else { 1 });
    }
}

fn ejecutar(cli: Cli) -> Result<()> {
    let path = resolve(&cli.db, dirs_home().as_deref())?;
    // Si hay un `tags.json` junto a la base de datos, sus reglas reemplazan
    // por completo a la taxonomía por defecto de `tags::classify`. Debe
    // fijarse antes de cualquier operación que clasifique memorias (migrate,
    // save, serve).
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
                rep.migradas, rep.archivadas, rep.fallidas
            );
            if !rep.curated_failed.is_empty() {
                println!(
                    "curadas en status failed (no migradas, reimportar a mano):"
                );
                for (ext_id, title) in &rep.curated_failed {
                    println!("  {ext_id}  {title}");
                }
            }
            sincronizar_tras_escribir(&conn, &dir);
        }
        Cmd::Search { query, project, tag, limit, explain, budget } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::cargar(dir)?;
            let p = anathem::paquete::resolver_y_avisar(&conn, dir);
            let ctx = anathem::search::Contexto::de_lectura(p.as_ref(), cfg.peso_bm25);
            let q = anathem::search::Query {
                text: query, project, tags: tag, limit,
                budget_tokens: budget.unwrap_or(anathem::search::DEFAULT_BUDGET_TOKENS),
                // La CLI no recorta: `mem search` sigue mostrando el fragmento
                // completo, como siempre.
                extracto: None,
            };
            let hits = anathem::search::run_explained(&conn, &q, &ctx)?;
            if hits.is_empty() {
                // Sin esto la búsqueda vacía no imprime nada y el usuario no
                // puede distinguir "no hay nada" de "el binario está roto".
                let total: i64 = conn.query_row(
                    "select count(*) from memories where status = 'active'", [], |r| r.get(0))?;
                if total == 0 {
                    println!("Sin resultados: la memoria está vacía. Guardá algo con `mem save \"...\"`.");
                } else {
                    let plural = if total == 1 { "memoria activa" } else { "memorias activas" };
                    println!("Sin resultados entre {total} {plural}.");
                    if p.is_some() {
                        // Con capa vectorial activa la búsqueda ya no es puramente
                        // léxica: el consejo de antes ("usá los sustantivos
                        // exactos") dejaría de ser cierto y confundiría más de lo
                        // que ayuda.
                        println!("La búsqueda combina bm25 con similitud vectorial. Probá con otros términos o revisá `mem model status` si esperabas más cobertura.");
                    } else {
                        println!("La búsqueda es léxica: probá con sustantivos que estén escritos en la memoria, no con la forma en que lo preguntarías en voz alta.");
                    }
                }
                return Ok(());
            }
            for (i, e) in hits.iter().enumerate() {
                if explain {
                    // Un resultado puede haber entrado por bm25, por el vector, o
                    // por los dos: mostramos cada señal que exista para que se
                    // entienda por qué recuperador llegó, no solo el que ganó.
                    let señales = match (e.bm25, e.coseno) {
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
            // Cuando el proyecto se deriva en vez de pedirse, se informa: un
            // valor implícito que no se ve es el que después nadie entiende
            // por qué quedó mal.
            let explicito = project.is_some();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let project = anathem::proyecto::resolver(project, &cwd);
            let g = anathem::ingest::guardar_con_tema(&conn, &text, &project, tag, topic.as_deref(), None)?;
            let etiquetas = if g.tags.is_empty() { String::new() } else { format!(" [{}]", g.tags.join(", ")) };
            let como = if explicito { "" } else { " (detectado)" };
            println!("guardada memoria {} en {project}{como}{etiquetas}", g.id);
            if let Some(msg) = g.describir() {
                println!("{msg}");
            }
            sincronizar_tras_escribir(&conn, &dir);
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
                .unwrap_or_else(|_| editor_por_defecto(cfg!(windows)).to_string());
            let status = std::process::Command::new(&editor).arg(&tmp).status()?;
            if !status.success() {
                anyhow::bail!("{editor} salió con error, no se guardó nada");
            }
            let nuevo = std::fs::read_to_string(&tmp)?;
            std::fs::remove_file(&tmp).ok();
            if nuevo != body {
                // `replace_body` rechunkea, y el cascade se lleva los
                // vectores del texto viejo. Sin esto, los nuevos no los
                // reconstruía nadie: la memoria editada quedaba invisible
                // para la búsqueda vectorial hasta el próximo `mem reindex`.
                anathem::edit::replace_body(&conn, id, &nuevo)?;
                println!("memoria {id} actualizada");
                sincronizar_tras_escribir(&conn, &dir);
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
            sincronizar_tras_escribir(&conn, &dir);
        }
        Cmd::Eval { evalset, label, out } => {
            let evalset = evalset.unwrap_or_else(|| {
                path.parent().unwrap_or(std::path::Path::new(".")).join("evalset.json")
            });
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::cargar(dir)?;
            let p = anathem::paquete::resolver_y_avisar(&conn, dir);
            let ctx = anathem::search::Contexto::de_lectura(p.as_ref(), cfg.peso_bm25);
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
            let cfg = anathem::config::cargar(dir)?;
            anathem::mcp::serve(&conn, dir, &cfg)?;
        }
        Cmd::Supersede { nueva, vieja } => {
            let conn = anathem::db::open(&path)?;
            anathem::supersede::link(&conn, nueva, vieja)?;
            println!("memoria {vieja} marcada como superseded por {nueva}");
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
            let temas = anathem::maint::temas(&conn)?;
            if temas.is_empty() {
                println!("Sin claves de tema. Se crean guardando con `mem save \"...\" --topic <clave>`.");
            } else {
                let ancho_p = temas.iter().map(|t| t.project.chars().count()).max().unwrap_or(0).max("proyecto".len());
                let ancho_t = temas.iter().map(|t| t.topic.chars().count()).max().unwrap_or(0).max("tema".len());
                println!("{:<ancho_p$}  {:<ancho_t$}  {:>8}  vigente", "proyecto", "tema", "memorias");
                for t in temas {
                    let vigente = t.vigente.map_or("ninguna".to_string(), |v| v.to_string());
                    println!("{:<ancho_p$}  {:<ancho_t$}  {:>8}  {vigente}", t.project, t.topic, t.total);
                }
            }
        }
        Cmd::History { id } => {
            let conn = anathem::db::open(&path)?;
            let cadena = anathem::maint::history(&conn, id)?;
            for (i, title, status) in cadena {
                println!("{i:5}  [{status:10}]  {title}");
            }
        }
        Cmd::Doctor => {
            // Sin `db::open`: ese abre para escribir y crea lo que falte, y un
            // diagnóstico no puede modificar lo que diagnostica.
            let informe = anathem::doctor::diagnosticar(&path);
            print!("{}", informe.render());
            std::io::Write::flush(&mut std::io::stdout())?;
            if informe.problemas() > 0 {
                std::process::exit(anathem::doctor::SALIDA_PROBLEMAS);
            }
        }
        Cmd::Model { accion } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            match accion {
                ModelCmd::Install { archivo } => {
                    let i = anathem::paquete::instalar(&conn, &dir, &archivo)?;
                    println!(
                        "paquete {} instalado ({} dimensiones), {} fragmentos indexados",
                        i.nombre, i.dims, i.fragmentos
                    );
                    if i.desplazada {
                        // El archivo quedó instalado, pero otro proceso se
                        // adueñó del registro mientras este indexaba y el
                        // índice quedó a medias. Decirlo: es lo único que
                        // distingue "instalé y funciona" de "instalé y el
                        // que manda es otro".
                        eprintln!(
                            "aviso: otro proceso cambió el paquete de vocabulario mientras se \
                             indexaba; esta instalación quedó desplazada y el índice a medias. \
                             Mirá `mem model status` y reinstalá el que quieras que quede."
                        );
                    }
                }
                ModelCmd::Status => match anathem::vecstore::pack_registrado(&conn)? {
                    None => println!("sin paquete: la búsqueda usa solo bm25"),
                    Some((nombre, sha, dims)) => {
                        let (con, tot) = anathem::vecstore::cobertura(&conn, dims)?;
                        let ruta = anathem::paquete::ruta(&dir, &nombre);
                        println!("paquete {nombre} ({dims} dimensiones, sha {})", &sha[..16.min(sha.len())]);
                        println!("cobertura: {con}/{tot} fragmentos");
                        if !ruta.exists() {
                            println!("FALTA el archivo {}: la búsqueda está degradada a bm25", ruta.display());
                        } else if con < tot {
                            println!("hay {} fragmentos sin vector usable; corré `mem reindex`", tot - con);
                        }
                        // `status` lee `meta` directo, sin pasar por
                        // `resolver`, así que es el único comando que puede
                        // ver la marca sin borrarla. Vale decirlo: mientras
                        // esté puesta, lo que informan las líneas de arriba
                        // describe al paquete de antes de la instalación
                        // interrumpida, no al archivo que está en disco.
                        if anathem::vecstore::instalacion_pendiente(&conn)?.is_some() {
                            println!(
                                "hay una instalación de {nombre} a medias: el archivo en disco \
                                 puede no ser este. La próxima búsqueda reindexa."
                            );
                        }
                    }
                },
                ModelCmd::Remove => match anathem::paquete::quitar(&conn, &dir)? {
                    Some(nombre) => {
                        println!("paquete {nombre} quitado; la búsqueda vuelve a ser solo bm25")
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
    fn resolve_sin_home_no_panica_devuelve_err() {
        let err = resolve("~/.anathem/mem.db", None)
            .expect_err("sin HOME, resolve debe fallar en vez de panicar");
        assert!(err.to_string().contains("HOME"), "mensaje inesperado: {err}");
    }

    #[test]
    fn resolve_con_home_expande_el_prefijo() {
        let home = Path::new("/home/alguien");
        let resuelto = resolve("~/.anathem/mem.db", Some(home)).unwrap();
        assert_eq!(resuelto, home.join(".anathem/mem.db"));
    }

    #[test]
    fn resolve_sin_prefijo_deja_la_ruta_intacta() {
        let resuelto = resolve("/tmp/x.db", None).unwrap();
        assert_eq!(resuelto, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn elegir_home_fuera_de_windows_prefiere_home_sobre_userprofile() {
        let h = elegir_home(
            Some(PathBuf::from("/home/alguien")),
            Some(PathBuf::from(r"C:\Users\alguien")),
            false,
        );
        assert_eq!(h, Some(PathBuf::from("/home/alguien")));
    }

    #[test]
    fn elegir_home_en_windows_prefiere_userprofile_sobre_home() {
        // Git Bash y MSYS definen HOME también en Windows, pero en formato
        // posix (`/c/Users/...`), que un binario nativo no entiende como
        // ruta de usuario. USERPROFILE es siempre la nativa: tiene que
        // ganar ella, aunque las dos estén definidas.
        let h = elegir_home(
            Some(PathBuf::from("/c/Users/alguien")),
            Some(PathBuf::from(r"C:\Users\alguien")),
            true,
        );
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn elegir_home_usa_userprofile_si_no_hay_home() {
        // El caso de Windows sin Git Bash: HOME no está definida y hay que
        // caer a USERPROFILE en vez de fallar.
        let h = elegir_home(None, Some(PathBuf::from(r"C:\Users\alguien")), false);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn elegir_home_en_windows_cae_a_home_si_no_hay_userprofile() {
        // USERPROFILE gana cuando las dos están, pero si falta ella sola
        // sigue habiendo un home utilizable en HOME.
        let h = elegir_home(Some(PathBuf::from(r"C:\Users\alguien")), None, true);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn elegir_home_home_vacia_no_cuenta_como_definida() {
        // HOME puede existir como variable y estar vacía (export HOME=).
        // Sin este chequeo, `resolve` uniría el resto de la ruta contra ""
        // y el resultado sería relativo al directorio actual, en silencio.
        let h = elegir_home(Some(PathBuf::new()), Some(PathBuf::from(r"C:\Users\alguien")), false);
        assert_eq!(h, Some(PathBuf::from(r"C:\Users\alguien")));
    }

    #[test]
    fn elegir_home_userprofile_vacia_no_cuenta_como_definida() {
        let h = elegir_home(Some(PathBuf::from("/home/alguien")), Some(PathBuf::new()), true);
        assert_eq!(h, Some(PathBuf::from("/home/alguien")));
    }

    #[test]
    fn elegir_home_sin_ninguna_de_las_dos_devuelve_none() {
        assert_eq!(elegir_home(None, None, false), None);
        assert_eq!(elegir_home(None, None, true), None);
    }

    #[test]
    fn editor_por_defecto_en_windows_es_notepad() {
        assert_eq!(editor_por_defecto(true), "notepad");
    }

    #[test]
    fn editor_por_defecto_fuera_de_windows_es_vi() {
        assert_eq!(editor_por_defecto(false), "vi");
    }

    #[test]
    fn resolve_sin_home_nombra_las_dos_variables() {
        // Más fuerte que la afirmación original: no alcanza con que el
        // mensaje mencione HOME, tiene que mencionar también la alternativa
        // de Windows, porque ahora el fallback existe y quien lea el error
        // en esa plataforma necesita saber que USERPROFILE es la que hay
        // que definir.
        let err = resolve("~/.anathem/mem.db", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HOME"), "mensaje inesperado: {msg}");
        assert!(msg.contains("USERPROFILE"), "mensaje inesperado: {msg}");
    }
}
