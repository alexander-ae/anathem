use anathem::permissions::restrict_to_owner;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "mem",
    about = "anathem — local memory for agents",
    version = anathem::doctor::FULL_VERSION
)]
struct Cli {
    /// Path to the database
    #[arg(long, global = true, default_value = "~/.anathem/mem.db")]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Creates the database if it does not exist and prints its path
    Init,
    /// Imports the frozen supermemory export
    Migrate {
        /// Path to the exported JSONL
        #[arg(long, default_value = "data/supermemory-export.jsonl")]
        from: PathBuf,
    },
    /// Searches the memory
    Search {
        /// The query text
        query: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        /// Narrows the search, as key:value. Repeatable, and they combine with AND
        ///
        /// The keys are `project`, `tag` and `topic`:
        /// `--scope project:api --scope tag:decision`. A key it does not
        /// know is an error, never a filter quietly ignored.
        #[arg(long = "scope", value_name = "KEY:VALUE")]
        scope: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Shows the raw bm25 score next to each result
        #[arg(long)]
        explain: bool,
        /// Token ceiling for the answer. 1200 by default, which is what
        /// gets injected into an agent's context. Raising it is useful to
        /// inspect more candidates than would otherwise fit.
        #[arg(long)]
        budget: Option<usize>,
    },
    /// Saves a new memory
    Save {
        /// The memory's body, or `-` to read it from stdin
        text: String,
        /// Which project it belongs to. Without this it is derived from
        /// the repository you are standing in: the name of the `origin`
        /// remote, or of the repo's folder if there is no remote.
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        /// Topic key. If the project already has an active memory under
        /// this key, the new one replaces it (left superseded, not deleted
        /// and not edited). Use it when what you are saving is an updated
        /// version of something already saved. See the existing ones with
        /// `mem topics`.
        #[arg(long)]
        topic: Option<String>,
        /// This memory was sent by another agent or another machine
        ///
        /// It is saved as a draft, invisible to search until someone runs
        /// `mem accept <id>`. Requires --from and --project.
        #[arg(long)]
        external: bool,
        /// Who sent it: a free label such as `claude-code@my-laptop`
        #[arg(long = "from", value_name = "LABEL", requires = "external")]
        from: Option<String>,
    },
    /// Edits an existing memory's body in $EDITOR
    Edit {
        /// id of the memory to edit
        id: i64,
    },
    /// Exports the memory to markdown files with frontmatter
    Export {
        #[arg(long, default_value = "export")]
        out: PathBuf,
    },
    /// Rebuilds chunks and the FTS5 index from memories.body
    Reindex,
    /// What the search log says about whether this is earning its place
    Stats {
        /// How far back: 7d, 30d, 6h. Everything, by default.
        #[arg(long)]
        since: Option<String>,
        /// How many of the queries that found nothing to list
        #[arg(long, default_value_t = 10)]
        empty: usize,
    },
    /// Looks for a better `bm25_weight` for this corpus, using the topic
    /// keys as its eval set. It recommends and changes nothing.
    Tune {
        /// Writes the recommended value into `config.json`, keeping every
        /// other key it already has
        #[arg(long)]
        apply: bool,
    },
    /// Runs the evaluation harness
    Eval {
        /// `evalset.json` next to the database, by default. Relative to
        /// the current directory would be fragile: running the same command
        /// from another folder would silently change what it measures
        /// against.
        #[arg(long)]
        evalset: Option<PathBuf>,
        #[arg(long, default_value = "anathem bm25")]
        label: String,
        /// Writes the results JSON to this file
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Starts the MCP server over stdio
    Serve,
    /// Marks a memory as replaced by another: the old one stops showing up in searches
    Supersede {
        /// id of the new memory that replaces the old one
        newer: i64,
        /// id of the old memory that is left superseded
        older: i64,
    },
    /// Pins a memory so it survives the candidate trimming in search
    Pin {
        id: i64,
    },
    /// Removes a memory's pin
    Unpin {
        id: i64,
    },
    /// Takes a memory out of search without deleting it
    Forget {
        id: i64,
    },
    /// Memories nobody has opened, to start a review pass from
    Stale {
        /// Only this project; by default, every one
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Which transcript a memory was written down in, and on what line
    Origin {
        id: i64,
        /// Where to look. Repeatable; by default `~/.claude/projects`
        #[arg(long = "in", value_name = "DIR")]
        roots: Vec<std::path::PathBuf>,
    },
    /// Marks a memory as checked against the world, not merely written down
    Verify {
        id: i64,
        /// Takes the mark away instead of putting it
        #[arg(long)]
        undo: bool,
    },
    /// Lists the topic keys: project, how many memories each had, and which is current
    Topics,
    /// What a topic says today, and what it said before
    ///
    /// `topics` is the index; this is the content. It prints the body of the
    /// version that answers for the key right now, and below it every
    /// version the key has had — ids, dates and states, no bodies.
    Show {
        /// A memory's id. Give this or --topic, not both
        id: Option<i64>,
        /// The topic key
        #[arg(long = "topic", value_name = "KEY", conflicts_with = "id")]
        topic: Option<String>,
        /// Project; by default the one of the repository you are standing in
        #[arg(long)]
        project: Option<String>,
    },
    /// Accepts a memory that arrived from elsewhere, making it searchable
    ///
    /// A received memory is saved as a draft on purpose: it is text another
    /// agent wrote, and nothing injects it into a model's context before a
    /// person has read it. `mem show <id>` is how you read one.
    Accept {
        /// The memory's id. Give this, or --all with --from
        id: Option<i64>,
        /// Accept every draft waiting from one sender
        #[arg(long, conflicts_with = "id")]
        all: bool,
        /// Which sender, when using --all
        #[arg(long = "from", value_name = "LABEL", requires = "all")]
        from: Option<String>,
    },
    /// What was saved lately, newest first, and who saved it
    ///
    /// Search ranks by relevance; this ranks by date. Useful for picking
    /// work back up, or for seeing what another agent left before following
    /// on from it.
    Recent {
        /// Project; by default the one of the repository you are standing in
        #[arg(long)]
        project: Option<String>,
        /// All projects, not just one
        #[arg(long = "all", alias = "todos", conflicts_with = "project")]
        all_projects: bool,
        /// How far back: 6h, 90m, 2d. 24h by default; `--since all` does not bound it
        #[arg(long = "since", alias = "desde", value_name = "SINCE", default_value = "24h")]
        since: String,
        /// At most this many memories
        #[arg(long = "limit", alias = "limite", value_name = "LIMIT", default_value_t = 15)]
        limit: usize,
        /// Only memories received from another agent, drafts included
        ///
        /// This is how you see what is waiting: a received memory is a
        /// draft until `mem accept` lets it in, so the usual listing —
        /// which shows active memories — hides exactly the ones you would
        /// want to look at.
        #[arg(long)]
        external: bool,
    },
    /// Shows a memory's supersession chain, newest to oldest
    History {
        id: i64,
    },
    /// What the MCP server searched for, newest first
    ///
    /// Every search through the server records what was asked, what came
    /// back, what it cost and whether the agent then opened one of the
    /// results. Searches run from this CLI are not recorded.
    ///
    /// It looks across every project by default, unlike `recent`: the
    /// server's searches often carry no project at all, and narrowing to
    /// the one of the repository you are standing in would hide most of
    /// them.
    Log {
        /// Only this project's searches
        #[arg(long)]
        project: Option<String>,
        /// How far back: 6h, 90m, 2d. 24h by default; `--since all` does not bound it
        #[arg(long = "since", value_name = "SINCE", default_value = "24h")]
        since: String,
        /// At most this many entries
        #[arg(long = "limit", value_name = "LIMIT", default_value_t = 20)]
        limit: usize,
        /// Deletes entries older than this window and says how many went
        ///
        /// There is no default and nothing trims the log on its own:
        /// retrieval history goes because someone asked for it with a
        /// number.
        #[arg(long = "prune", value_name = "WINDOW")]
        prune: Option<String>,
    },
    /// Which searches returned a memory, where in the ranking, and whether it was opened
    Why {
        id: i64,
    },
    /// Imports what `mem export` wrote: restoring a backup, moving to
    /// another machine, or syncing two computers by hand
    Import {
        /// The folder `mem export --out` wrote
        #[arg(long = "from", alias = "desde", value_name = "FROM")]
        from: PathBuf,
        /// Import onto a database that already has memories, adding only
        /// what is missing
        #[arg(long = "merge", alias = "fusionar")]
        merge: bool,
    },
    /// Checks whether a newer version has been published
    ///
    /// It touches the network only when you ask: anathem talks to no
    /// server on its own. It neither downloads nor replaces anything; it
    /// tells you what to run.
    Update,
    /// Diagnoses the installation without changing anything
    ///
    /// Exits with 0 if there are no problems (warnings and information do
    /// not count), with 1 if there are, and with 2 if it could not run.
    Doctor,
    /// Manages the vector layer's vocabulary pack
    Model {
        #[command(subcommand)]
        action: ModelCmd,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    /// Installs a pack from a local file and reindexes
    Install {
        /// Path to the .anathem-pack file
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Installs a pack that has no .sha256 beside it
        ///
        /// Every pack the builder writes and every pack a release publishes
        /// carries one, so its absence means the file travelled without it.
        /// This says you know where the pack came from and want it anyway.
        #[arg(long)]
        no_verify: bool,
    },
    /// Shows the active pack and the vector coverage
    Status,
    /// Removes the pack and its vectors; search goes back to bm25 only
    Remove,
}

/// Why a log can be empty, or a memory missing from it. Said wherever the
/// answer is "nothing", which is the one case where the reason matters.
const ONLY_MCP_IS_LOGGED: &str =
    "Only searches through the MCP server are logged; the ones run from this CLI are not.";

/// Shortens a stored instant to what a listing can read.
///
/// `created_ts` is a full RFC 3339 timestamp down to the nanosecond, and
/// `created` is a bare date; `coalesce` hands back either. Nanoseconds and a
/// zone offset are noise in a column meant to be skimmed, and the minute is
/// the resolution that matters — two versions of one topic saved the same
/// afternoon are exactly the case `mem recent` already says a day cannot
/// tell apart.
///
/// Cut by characters rather than parsed: anything unexpected comes back
/// shortened instead of turning into a wrong date, and a bare date is left
/// alone.
fn short_when(raw: &str) -> String {
    if raw.len() <= 10 {
        return raw.to_string();
    }
    raw.chars().take(16).collect::<String>().replacen('T', " ", 1).trim_end().to_string()
}

/// Resolves a `~/` prefix against `home`, without touching the environment —
/// which makes it testable without depending on global process variables.
fn resolve(p: &str, home: Option<&Path>) -> Result<PathBuf> {
    match p.strip_prefix("~/") {
        Some(rest) => {
            let home = home
                .ok_or_else(|| anyhow::anyhow!("neither HOME nor USERPROFILE is defined"))?;
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
        Ok(SyncOutcome::Complete(n)) => println!("new vectors: {n}"),
        // Another process installed or removed the pack while this one was
        // embedding. What was written before the change its `adopt_pack`
        // already deleted, and what was left was never written: some chunks
        // end up without a vector, which is the acceptable degradation.
        // Mixing them would not be. The command still exits 0: the memory
        // is saved.
        Ok(SyncOutcome::Abandoned(_)) => eprintln!(
            "warning: the memory was saved, but another process changed the vocabulary \
             pack while the vectors were being computed; the batch was abandoned so two \
             packs are not mixed. Run `mem reindex` when you can."
        ),
        Err(e) => eprintln!(
            "warning: the memory was saved but the vectors could not be updated ({e}); \
             run `mem reindex` when you can."
        ),
    }
}

/// Dies from `SIGPIPE` the way `grep` and `cat` do, instead of panicking.
///
/// Rust sets `SIGPIPE` to `SIG_IGN` before `main` runs, so a write to a
/// closed pipe returns `EPIPE` and `println!` — which unwraps — panics on
/// it. For a library that is the safe default; for a command line tool it
/// means `mem search x | head -1` ends in a panic and a backtrace note,
/// after the user did something entirely ordinary.
///
/// This also changes how `serve` ends when its MCP client disappears: today
/// the `writeln!` returns `EPIPE`, which travels up as an error and prints
/// `Error: Broken pipe`. With the default handler back the process is killed
/// by the signal instead. That is the better of the two — the message was
/// being written to a client that is already gone — but it is a change, not
/// only a fix.
#[cfg(unix)]
fn die_on_closed_pipe() {
    // SAFETY: `signal` with `SIG_DFL` is async-signal-safe, and this runs
    // before any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn die_on_closed_pipe() {}

fn main() {
    die_on_closed_pipe();
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
            println!("database ready at {}", path.display());
        }
        Cmd::Migrate { from } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let rep = anathem::migrate::run(&conn, &from)?;
            println!(
                "migrated={} archived={} failed={}",
                rep.migrated, rep.archived, rep.failed
            );
            if !rep.curated_failed.is_empty() {
                println!(
                    "curated ones with status failed (not migrated, reimport by hand):"
                );
                for (ext_id, title) in &rep.curated_failed {
                    println!("  {ext_id}  {title}");
                }
            }
            sync_after_write(&conn, &dir);
        }
        Cmd::Search { query, project, tag, scope, limit, explain, budget } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            let p = anathem::packstore::resolve_and_warn(&conn, dir);
            let ctx = anathem::search::SearchContext::for_reading(p.as_ref(), cfg.bm25_weight);
            let scopes = anathem::search::parse_scopes(&scope)?;
            let q = anathem::search::Query {
                text: query, project, tags: tag, topic: None, limit,
                budget_tokens: budget.unwrap_or(anathem::search::DEFAULT_BUDGET_TOKENS),
                // The CLI does not trim: `mem search` still shows the whole
                // chunk, as it always has, and for the same reason it shows
                // no outline: a person reading the terminal is not paying a
                // token budget for it.
                excerpt: None,
                outline: false,
            };
            let q = scopes.onto(q)?;
            let hits = anathem::search::run_explained(&conn, &q, &ctx)?;
            if hits.is_empty() {
                // An empty scope is its own answer. "No results among 300
                // memories" would send someone rephrasing a query that was
                // never the problem: nothing is filed under that scope.
                if anathem::search::scope_is_empty(&conn, &q)? {
                    println!(
                        "No memories match scope {}. Nothing is filed under it, so other terms will not help.",
                        anathem::search::describe_scope(&q)
                    );
                    return Ok(());
                }
                // Without this an empty search prints nothing and the user
                // cannot tell "there is nothing" from "the binary is broken".
                let total: i64 = conn.query_row(
                    "select count(*) from memories where status = 'active'", [], |r| r.get(0))?;
                if total == 0 {
                    println!("No results: the memory is empty. Save something with `mem save \"...\"`.");
                } else {
                    let plural = if total == 1 { "active memory" } else { "active memories" };
                    println!("No results among {total} {plural}.");
                    if p.is_some() {
                        // With the vector layer active, search is no longer
                        // purely lexical: the old advice ("use the exact
                        // nouns") would stop being true and would confuse
                        // more than it helps.
                        println!("Search fuses bm25 with vector similarity. Try other terms, or check `mem model status` if you expected more coverage.");
                    } else {
                        println!("Search is lexical: try nouns that are likely written in the memory, not the way you would ask it out loud.");
                    }
                }
                return Ok(());
            }
            for (i, e) in hits.iter().enumerate() {
                // A memory somebody else sent is marked here too, not only
                // in what MCP injects: a person reading results deserves the
                // same distinction an agent gets.
                let from: String = conn
                    .query_row(
                        "select received_from from memories where id = ?1",
                        [e.hit.memory_id],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .unwrap_or(None)
                    .map(|f| format!(" [from: {f}]"))
                    .unwrap_or_default();
                if explain {
                    // A result may have come in through bm25, through the
                    // vector, or through both: we show every signal there is,
                    // so it is clear which retriever brought it in, not only
                    // the one that won.
                    let signals = match (e.bm25, e.cosine) {
                        (Some(b), Some(c)) => format!("bm25={b:.3} cosine={c:.3}"),
                        (Some(b), None) => format!("bm25={b:.3}"),
                        (None, Some(c)) => format!("cosine={c:.3}, vector only"),
                        (None, None) => "no signal".to_string(),
                    };
                    // The raw scores do not say how much of the question
                    // landed: a hit scored on one common word looks the same
                    // as one that matched every term. This is the number
                    // that told them apart.
                    let signals = format!("{signals}, {}/{} words", e.words_matched, e.words_total);
                    println!("{:2}. [{}]{} {}  ({signals})", i + 1, e.hit.project, from, e.hit.title);
                } else {
                    println!("{:2}. [{}]{} {}", i + 1, e.hit.project, from, e.hit.title);
                }
                println!("    {}", e.hit.snippet.chars().take(180).collect::<String>());
            }
        }
        Cmd::Save { text, project, tag, topic, external, from } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            // `-` means the body comes on stdin, which is what lets a note
            // travel over SSH without going through a shell argument:
            //   ssh host mem save - --external --from me --project p < note.md
            let text = if text == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                buf
            } else {
                text
            };
            if external {
                let Some(from) = from else {
                    anyhow::bail!("--external needs --from <label>: a received memory with no \
                                   sender records nothing worth recording");
                };
                let Some(project) = project else {
                    anyhow::bail!("--external needs --project: deriving it from the directory you \
                                   are standing in would file someone else's note under whatever \
                                   repository happens to be here");
                };
                if topic.is_some() {
                    anyhow::bail!("--topic cannot be used with --external: a topic key replaces \
                                   the current version of a subject, and a memory from another \
                                   machine replacing a local one needs a rule about which of the \
                                   two is current. Save it without a key and use `mem supersede` \
                                   after accepting it, if that is what you meant.");
                }
                let g = anathem::ingest::save_received(&conn, &text, &project, tag, &from)?;
                println!(
                    "received memory {} in {project}, from {from}, saved as a draft",
                    g.id
                );
                println!("Nothing searches it until you accept it: `mem show {}` to read it, `mem accept {}` to let it in.", g.id, g.id);
                sync_after_write(&conn, &dir);
                return Ok(());
            }
            // When the project is derived instead of given, say so: an
            // implicit value nobody sees is the one nobody can later explain
            // the wrongness of.
            let explicit = project.is_some();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let project = anathem::project::resolve(project, &cwd);
            let g = anathem::ingest::save_with_topic(&conn, &text, &project, tag, topic.as_deref(), None)?;
            let labels = if g.tags.is_empty() { String::new() } else { format!(" [{}]", g.tags.join(", ")) };
            let how = if explicit { "" } else { " (detected)" };
            println!("saved memory {} in {project}{how}{labels}", g.id);
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
            // The temporary used to be `temp_dir()/anathem-edit-{id}.md`: a
            // fixed, guessable name in a world-writable directory, created
            // with the default umask and without O_EXCL. On a shared host
            // that is two problems — the memory's body was readable by any
            // other user while the editor had it open, and whoever got
            // there first could leave a symlink at that path, so the write
            // landed on a file of theirs and what came back into the
            // database was their text, not the user's.
            //
            // A private directory, 0700, holding a 0600 file with a random
            // name closes both. Both the directory and the file are removed
            // when `carpeta` drops, which — unlike the old `remove_file` on
            // the happy path — also covers the editor exiting with an
            // error.
            let carpeta = tempfile::Builder::new().prefix("anathem-edit-").tempdir()?;
            restrict_to_owner(carpeta.path())?;
            let tmp = carpeta.path().join(format!("{id}.md"));
            std::fs::write(&tmp, &body)?;
            restrict_to_owner(&tmp)?;
            let editor = std::env::var("EDITOR")
                .unwrap_or_else(|_| default_editor(cfg!(windows)).to_string());
            let status = std::process::Command::new(&editor).arg(&tmp).status()?;
            if !status.success() {
                anyhow::bail!("{editor} exited with an error, nothing was saved");
            }
            let new_id = std::fs::read_to_string(&tmp)?;
            if new_id != body {
                // `replace_body` re-chunks, and the cascade takes the old
                // text's vectors with it. Without this, nobody rebuilt the
                // new ones: the edited memory stayed invisible to vector
                // search until the next `mem reindex`.
                anathem::edit::replace_body(&conn, id, &new_id)?;
                println!("memory {id} updated");
                sync_after_write(&conn, &dir);
            } else {
                println!("no changes");
            }
        }
        Cmd::Export { out } => {
            let conn = anathem::db::open(&path)?;
            let n = anathem::export::to_markdown(&conn, &out)?;
            println!("exported {n} memories to {}", out.display());
        }
        Cmd::Reindex => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let n = anathem::edit::reindex(&conn)?;
            println!("reindexed {n} memories");
            sync_after_write(&conn, &dir);
        }
        Cmd::Stats { since, empty } => {
            let conn = anathem::db::open(&path)?;
            let cutoff = anathem::stats::cutoff_for(since.as_deref())?;
            let s = anathem::stats::collect(&conn, cutoff.as_deref())?;
            if s.searches == 0 {
                println!("No searches logged{}.\n\nOnly the MCP server writes to the log; \
                          searches run from this CLI are not recorded.",
                    since.as_deref().map(|w| format!(" in the last {w}")).unwrap_or_default());
                return Ok(());
            }
            println!(
                "{} searches{}, {} to {}.\n",
                s.searches,
                since.as_deref().map(|w| format!(" in the last {w}")).unwrap_or_default(),
                s.first.as_deref().unwrap_or("?").chars().take(10).collect::<String>(),
                s.last.as_deref().unwrap_or("?").chars().take(10).collect::<String>(),
            );
            println!(
                "This is not recall. It cannot say whether the memory you wanted existed\n\
                 and was missed — only what happened to the searches that ran.\n"
            );
            println!("  answered         {:>6}  ({:.0}% of searches ended in opening a memory)",
                s.followed_up, s.answer_rate() * 100.0);
            println!("  found nothing    {:>6}  ({:.0}%)",
                s.empty, s.empty as f64 / s.searches as f64 * 100.0);
            println!("  tokens           {:>6}  in all, {} per search at the median",
                s.tokens_total, s.tokens_p50);
            match s.median_rank() {
                Some(r) if s.ranks.len() >= 20 => println!(
                    "  opened at rank   {:>6}  at the median, over {} searches that recorded one",
                    r, s.ranks.len()),
                _ => println!(
                    "  opened at rank        -  {} searches recorded which one, too few to rank",
                    s.ranks.len()),
            }
            if s.by_week.len() > 1 {
                println!("\n  week starting   searches   answered");
                for (w, n, f) in &s.by_week {
                    println!("  {w}   {n:>8}   {f:>8}");
                }
            }
            if !s.empty_queries.is_empty() {
                println!("\nFound nothing. These are the wordings to improve, and they are free:");
                for q in s.empty_queries.iter().take(empty) {
                    println!("  {q}");
                }
                if s.empty_queries.len() > empty {
                    println!("  ... and {} more", s.empty_queries.len() - empty);
                }
            }
        }
        Cmd::Tune { apply } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new("."));
            let cfg = anathem::config::load(dir)?;
            let cases = anathem::tune::cases_from_topics(&conn)?;
            anathem::tune::check_enough(cases.len())?;
            let Some(pack) = anathem::packstore::resolve_and_warn(&conn, dir) else {
                anyhow::bail!(
                    "there is no vocabulary pack installed, and `bm25_weight` only \
                     balances the\nlexical ranking against the vector one. With no \
                     vector layer every weight\ngives the same answer, so there is \
                     nothing here to tune."
                );
            };

            println!(
                "Reading {} topic keys as the eval set.\n\n\
                 A topic key names its subject in keywords, so these questions are \
                 the\nkind bm25 already answers well. That makes the table below \
                 good for\nCHOOSING a weight — the bias falls on every row equally — \
                 and wrong to\nquote as a quality figure for anathem.\n",
                cases.len()
            );

            let scored = anathem::tune::sweep_until_interior(&conn, &cases, &pack)?;
            println!("{:>8}  {:>7}  {:>9}", "weight", "MRR", "recall@5");
            for (w, mrr, r5) in &scored {
                let here = if (*w - cfg.bm25_weight).abs() < f64::EPSILON { "  <- yours" } else { "" };
                println!("{w:>8}  {mrr:>7.3}  {:>8.0}%{here}", r5 * 100.0);
            }

            let pairs: Vec<(f64, f64)> = scored.iter().map(|(w, m, _)| (*w, *m)).collect();
            let best = anathem::tune::best_weight(&pairs);
            let now = pairs.iter().find(|(w, _)| (*w - cfg.bm25_weight).abs() < f64::EPSILON)
                .map(|(_, m)| *m);
            let best_mrr = pairs.iter().find(|(w, _)| *w == best).map(|(_, m)| *m).unwrap_or(0.0);
            println!();
            match now {
                Some(n) if (best - cfg.bm25_weight).abs() < f64::EPSILON =>
                    println!("Your `bm25_weight` of {} is already the best of these ({n:.3}).", cfg.bm25_weight),
                Some(n) => println!(
                    "`bm25_weight: {best}` scores {best_mrr:.3} against your {}'s {n:.3}.",
                    cfg.bm25_weight),
                None => println!(
                    "`bm25_weight: {best}` is the best of these ({best_mrr:.3}). Your {} was \
                     not on the\ngrid, so there is no row to compare it against.",
                    cfg.bm25_weight),
            }
            let ya_es_el_tuyo = (best - cfg.bm25_weight).abs() < f64::EPSILON;
            if best >= anathem::tune::MAX_WEIGHT || best <= anathem::tune::MIN_WEIGHT {
                println!(
                    "\nThe grid was extended until it ran out of room and the best \
                     value is still
at the end, which says the vector ranking is \
                     adding nothing on this corpus
rather than that {best} is a good \
                     number."
                );
            }
            if ya_es_el_tuyo {
                // Nothing to apply, and `--apply` should say so rather than
                // rewriting a file to the value it already holds.
            } else if apply {
                use anathem::config::Wrote;
                let cual = dir.join("config.json");
                match anathem::config::write_bm25_weight(dir, best)? {
                    Wrote::Created =>
                        println!("\n{}: created, holding bm25_weight {best}", cual.display()),
                    Wrote::KeyAdded =>
                        println!("\n{}: bm25_weight {best} added", cual.display()),
                    Wrote::Replaced(v) =>
                        println!("\n{}: bm25_weight {v} -> {best}", cual.display()),
                }
                println!(
                    "Every other key it had is untouched. To undo it, put the old \
                     value back
or delete the key: with none, the default is {}.",
                    anathem::config::Config::default().bm25_weight
                );
                println!(
                    "\nThis was measured against THIS corpus's topic keys, and issue \
                     #25 found
that the best weight does not carry over between \
                     corpora. Every figure
published in the README was measured at 1.0."
                );
            } else {
                println!(
                    "\nNothing here changed it. `mem tune --apply` writes it for you, \
                     keeping
every other key the file has, or put it in {} yourself:",
                    dir.join("config.json").display()
                );
                println!("\n    {{ \"bm25_weight\": {best} }}");
            }
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
            println!("memory {older} marked as superseded by {newer}");
        }
        Cmd::Pin { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::pin(&conn, id, true)?;
            println!("memory {id} pinned");
        }
        Cmd::Unpin { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::pin(&conn, id, false)?;
            println!("memory {id} is no longer pinned");
        }
        Cmd::Stale { project, limit } => {
            let conn = anathem::db::open(&path)?;
            let filas = anathem::stale::never_read(&conn, project.as_deref(), limit)?;
            if filas.is_empty() {
                println!("no active memories to show.");
                return Ok(());
            }
            for s in &filas {
                let cuando = match &s.last_accessed {
                    Some(t) => format!("last opened {}", t.get(..10).unwrap_or(t)),
                    None => "never opened".to_string(),
                };
                println!("id:{:<5} [{}] {:<12} {}", s.id, s.project, cuando, s.title);
            }
            // Said every time and not once in the docs: a list that looks
            // like "neglected memories" and is not would send somebody
            // rewriting notes that were read perfectly well.
            println!(
                "\n`last opened` is written by `get_memory`, which fetches a whole body. \
                 A memory\nread from the excerpt in a search result does not count, and \
                 most are: that is\nwhat the two-stage protocol is for. So this is where \
                 to start looking, not a list\nof neglected notes."
            );
        }
        Cmd::Origin { id, roots } => {
            let conn = anathem::db::open(&path)?;
            let body: String = match conn.query_row(
                "select body from memories where id = ?1", [id], |r| r.get::<_, String>(0)) {
                Ok(b) => b,
                Err(rusqlite::Error::QueryReturnedNoRows) =>
                    anyhow::bail!("there is no memory with id:{id}"),
                Err(e) => return Err(e.into()),
            };
            let Some(aguja) = anathem::stale::needle(&body) else {
                println!("id:{id} has no line long enough to search for.");
                return Ok(());
            };
            let roots = if roots.is_empty() { anathem::stale::default_roots() } else { roots };
            let hits = anathem::stale::find_origin(&roots, &aguja)?;
            if hits.is_empty() {
                // Not an error. A note typed by a person in their own
                // terminal is in no transcript, and saying so plainly is
                // more useful than a non-zero exit.
                println!(
                    "id:{id} is in no transcript under {}.\nIt was written outside an \
                     agent session, or the log has been deleted.",
                    roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")
                );
                return Ok(());
            }
            for o in &hits {
                println!("{}:{}", o.file.display(), o.line);
            }
            println!(
                "\nThe lines before that one are what the agent had just done when it \
                 decided to\nsave. Twenty of them came to about 10,000 tokens on the \
                 transcript measured."
            );
        }
        Cmd::Verify { id, undo } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::verify(&conn, id, !undo)?;
            if undo {
                println!("memory {id} is no longer marked as verified");
            } else {
                println!("memory {id} marked as verified");
            }
        }
        Cmd::Forget { id } => {
            let conn = anathem::db::open(&path)?;
            anathem::maint::forget(&conn, id)?;
            println!("memory {id} forgotten (still in the database, out of search)");
        }
        Cmd::Topics => {
            let conn = anathem::db::open(&path)?;
            let topics = anathem::maint::topics(&conn)?;
            if topics.is_empty() {
                println!("No topic keys. They are created by saving with `mem save \"...\" --topic <key>`.");
            } else {
                let width_p = topics.iter().map(|t| t.project.chars().count()).max().unwrap_or(0).max("project".len());
                let width_t = topics.iter().map(|t| t.topic.chars().count()).max().unwrap_or(0).max("topic".len());
                println!("{:<width_p$}  {:<width_t$}  {:>8}  current", "project", "topic", "memories");
                for t in topics {
                    let current_one = t.current_one.map_or("none".to_string(), |v| v.to_string());
                    println!("{:<width_p$}  {:<width_t$}  {:>8}  {current_one}", t.project, t.topic, t.total);
                }
            }
        }
        Cmd::Accept { id, all, from } => {
            let conn = anathem::db::open(&path)?;
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            match (id, all) {
                (Some(id), _) => {
                    let from = anathem::maint::accept(&conn, id)?;
                    println!("accepted memory {id}, received from {from}; searches can see it now");
                }
                (None, true) => {
                    // No sender, no batch. "Everything waiting, from
                    // wherever" is the decision this gate exists to make
                    // someone take on purpose.
                    let Some(from) = from else {
                        anyhow::bail!("--all needs --from <label>: accepting everything waiting, \
                                       whatever sent it, is the one decision this gate is for");
                    };
                    let n = anathem::maint::accept_all_from(&conn, &from)?;
                    println!("accepted {n} memories received from {from}");
                }
                (None, false) => {
                    anyhow::bail!("`mem accept` needs a memory id, or --all --from <label>");
                }
            }
            sync_after_write(&conn, &dir);
        }
        Cmd::Show { id: Some(id), .. } => {
            let conn = anathem::db::open(&path)?;
            let Some(m) = anathem::maint::memory(&conn, id)? else {
                anyhow::bail!("there is no memory with id:{id}");
            };
            println!("id: {}", m.id);
            println!("project: {}", m.project);
            println!("status: {}", m.status);
            let who = m
                .received_from
                .as_deref()
                .map(|f| format!(", received from {f}"))
                .or_else(|| m.agent.as_deref().map(|a| format!(", saved by {a}")))
                .unwrap_or_default();
            println!("saved: {}{who}", short_when(&m.when));
            if let Some(t) = &m.topic {
                println!("topic: {t}");
            }
            if m.status == "draft" {
                println!("Waiting: nothing searches it until `mem accept {id}`.");
            }
            println!();
            println!("{}", m.body.trim_end());
        }
        Cmd::Show { id: None, topic, project } => {
            let Some(topic) = topic else {
                anyhow::bail!("`mem show` needs a memory id or --topic <key>");
            };
            let conn = anathem::db::open(&path)?;
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = anathem::project::resolve(project, &cwd);
            match anathem::maint::topic_state(&conn, &project, &topic)? {
                anathem::maint::TopicLookup::Unknown(keys) => {
                    // Exit 0, like every other read command here: nothing
                    // went wrong, the answer is simply that the key is not
                    // there, and the list is the useful half of saying so.
                    println!("no topic \"{topic}\" in project {project}.");
                    if keys.is_empty() {
                        println!("That project has no topic keys yet. They are created by saving with `mem save \"...\" --topic <key>`.");
                    } else {
                        println!("Keys it does have:");
                        for k in keys {
                            println!("  {k}");
                        }
                    }
                }
                anathem::maint::TopicLookup::Found(state) => {
                    println!("topic: {}", state.topic);
                    println!("project: {}", state.project);
                    match &state.current {
                        Some(c) => {
                            let who = c.agent.as_deref().map_or(String::new(), |a| format!(" by {a}"));
                            println!("current: id:{}, saved {}{who}", c.id, short_when(&c.when));
                            println!();
                            println!("{}", c.body.trim_end());
                        }
                        None => println!(
                            "current: none — every version of this key was forgotten"
                        ),
                    }
                    println!();
                    println!("history, newest first:");
                    let width = state
                        .chain
                        .iter()
                        .map(|v| v.status.chars().count())
                        .max()
                        .unwrap_or(0);
                    for v in &state.chain {
                        println!("  id:{:<6} {}  [{:<width$}]  {}", v.id, short_when(&v.when), v.status, v.title);
                    }
                }
            }
        }
        Cmd::Import { from, merge } => {
            let conn = anathem::db::open(&path)?;
            let r = anathem::import::import_dir(&conn, &from, merge)?;
            println!(
                "imported {} memories, {} skipped as already present, {} supersession chains rebuilt",
                r.inserted, r.skipped, r.chains
            );
            if r.inserted > 0 {
                println!("If you have a vocabulary pack, run `mem reindex` for the vectors.");
            }
        }
        Cmd::Update => {
            anathem::update::check_for_update()?;
        }
        Cmd::Recent { project, all_projects, since, limit, external } => {
            let conn = anathem::db::open(&path)?;
            // `all` es lo que anuncia la ayuda; `todo` era el centinela
            // original y se sigue aceptando, igual que los alias de las
            // banderas, para no romper a quien lo tenga escrito.
            let unbounded = ["all", "todo"];
            let window = if unbounded.iter().any(|k| since.trim().eq_ignore_ascii_case(k)) {
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
            let items = anathem::recent::list_recent_filtered(
                &conn, project_.as_deref(), window, limit, cfg.excerpt, external)?;
            println!("{}", anathem::recent::format_items(&items, project_.is_none()));
        }
        Cmd::History { id } => {
            let conn = anathem::db::open(&path)?;
            let chain = anathem::maint::history(&conn, id)?;
            for (i, title, status) in chain {
                println!("{i:5}  [{status:10}]  {title}");
            }
        }
        Cmd::Log { project, since, limit, prune } => {
            let conn = anathem::db::open(&path)?;
            if let Some(window) = prune {
                let seconds = anathem::recent::duration_in_seconds(&window)?;
                let n = anathem::logsearch::prune(&conn, project.as_deref(), seconds)?;
                println!("removed {n} log entries older than {window}");
            } else {
                let unbounded = ["all", "todo"];
                let window = if unbounded.iter().any(|k| since.trim().eq_ignore_ascii_case(k)) {
                    None
                } else {
                    Some(anathem::recent::duration_in_seconds(&since)?)
                };
                let entries =
                    anathem::logsearch::list(&conn, project.as_deref(), window, limit)?;
                if entries.is_empty() {
                    println!("no searches logged in this window. {ONLY_MCP_IS_LOGGED}");
                }
                for e in entries {
                    let opened = match e.followed {
                        None => "none".to_string(),
                        Some(anathem::logsearch::Followed::Memory(i)) => i.to_string(),
                        // An older row knows one was opened and not which.
                        Some(anathem::logsearch::Followed::Unrecorded) => "yes".to_string(),
                    };
                    println!(
                        "{}  project={}  returned={}  tokens={}  opened={opened}  query=\"{}\"",
                        short_when(&e.ts),
                        e.project.as_deref().unwrap_or("-"),
                        e.returned.len(),
                        e.budget_used,
                        e.query,
                    );
                }
            }
        }
        Cmd::Why { id } => {
            let conn = anathem::db::open(&path)?;
            let appearances = anathem::logsearch::why(&conn, id, 50)?;
            if appearances.is_empty() {
                println!("id:{id} never came back from a logged search.");
                println!("{ONLY_MCP_IS_LOGGED}");
            } else {
                println!("id:{id} came back from {} logged searches, newest first:", appearances.len());
                for a in appearances {
                    println!(
                        "{}  position {} of {}  project={}  opened={}  query=\"{}\"",
                        short_when(&a.ts),
                        a.position,
                        a.of,
                        a.project.as_deref().unwrap_or("-"),
                        if a.opened { "yes" } else { "no" },
                        a.query,
                    );
                }
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
                ModelCmd::Install { file, no_verify } => {
                    let sidecar = if no_verify {
                        anathem::packstore::Sidecar::Optional
                    } else {
                        anathem::packstore::Sidecar::Required
                    };
                    let i = anathem::packstore::install(&conn, &dir, &file, sidecar)?;
                    println!(
                        "pack {} installed ({} dimensions), {} chunks indexed",
                        i.name, i.dims, i.chunks
                    );
                    if i.displaced {
                        // The file was installed, but another process took
                        // over the registry while this one was indexing and
                        // the index was left half done. Say it: it is the
                        // only thing separating "installed and working" from
                        // "installed, and someone else is in charge".
                        eprintln!(
                            "warning: another process changed the vocabulary pack while this \
                             one was indexing; this install was displaced and the index left \
                             half done. Look at `mem model status` and reinstall whichever you \
                             want to keep."
                        );
                    }
                }
                ModelCmd::Status => match anathem::vecstore::registered_pack(&conn)? {
                    None => println!("no pack: search uses bm25 only"),
                    Some((name, sha, dims)) => {
                        let (covered, tot) = anathem::vecstore::coverage(&conn, dims)?;
                        let pack_path = anathem::packstore::pack_path(&dir, &name);
                        println!("pack {name} ({dims} dimensions, sha {})", &sha[..16.min(sha.len())]);
                        println!("coverage: {covered}/{tot} chunks");
                        if !pack_path.exists() {
                            println!("the file {} is MISSING: search is degraded to bm25", pack_path.display());
                        } else if covered < tot {
                            println!("{} chunks have no usable vector; run `mem reindex`", tot - covered);
                        }
                        // `status` reads `meta` directly, without going
                        // through `resolve`, so it is the only command that
                        // can see the mark without clearing it. Worth
                        // saying: while it is set, what the lines above
                        // report describes the pack from before the
                        // interrupted install, not the file on disk.
                        if anathem::vecstore::pending_install(&conn)?.is_some() {
                            println!(
                                "there is a half-done install of {name}: the file on disk may \
                                 not be this one. The next search reindexes."
                            );
                        }
                    }
                },
                ModelCmd::Remove => match anathem::packstore::remove_pack(&conn, &dir)? {
                    Some(name) => {
                        println!("pack {name} removed; search goes back to bm25 only")
                    }
                    None => println!("there was no pack installed"),
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
            .expect_err("with no HOME, resolve has to fail instead of panicking");
        assert!(err.to_string().contains("HOME"), "mensaje inesperado: {err}");
    }

    #[test]
    fn short_when_leaves_a_bare_date_alone() {
        assert_eq!(super::short_when("2026-10-28"), "2026-10-28");
    }

    #[test]
    fn short_when_cuts_a_timestamp_at_the_minute() {
        assert_eq!(
            super::short_when("2026-11-05T09:02:40.305529008-05:00"),
            "2026-11-05 09:02"
        );
    }

    #[test]
    fn short_when_does_not_invent_a_date_out_of_something_it_cannot_read() {
        // Shortened, never reinterpreted: a wrong date that looks right is
        // worse than an odd-looking string.
        assert_eq!(super::short_when("no es una fecha en absoluto"), "no es una fecha");
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
