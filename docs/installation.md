# Installation and connecting it to the agents

## 0. The short way

Nothing has to be compiled and Rust does not have to be installed:

```bash
curl -fsSL https://gitlab.com/__alexander__/anathem/-/raw/main/scripts/install.sh -o install.sh
sh install.sh --setup-path
```

The script detects your platform, downloads the binary from the latest
release, verifies its sha256, installs it, sets up your shell's `PATH`,
creates the database and downloads the en-es vocabulary pack. At the end it
runs `mem doctor` so you can see that it came out healthy. It is idempotent:
running it again breaks nothing.

The script is written to a file instead of being piped into `sh` on purpose.
`curl ... | sh` blindly executes whatever the server answers; with the file
you can read it first, and it is 417 lines of `sh` with nothing strange in
them.

There are binaries for Linux x86_64, macOS arm64 (M1 onwards), macOS x86_64
and Windows x86_64. On any other platform the script says so and compiles from
source, which does need Rust and a C compiler.

Useful options:

```bash
# compile from source even though there is a binary (needs cloning the repo)
sh scripts/install.sh --setup-path --build

# use a vocabulary pack you already have on disk
sh install.sh --setup-path --pack ~/Downloads/en-es.anathem-pack

# no vector layer: lexical search only, nothing to download
sh install.sh --setup-path --no-pack

# pin a version instead of the latest one
sh install.sh --tag v1.0.0
```

Everything that gets downloaded is checked against its sha256, and if it
cannot be checked — because the release does not ship the `.sha256`, or
because the machine has neither `sha256sum` nor `shasum` — the installer stops
instead of carrying on. That catches a corrupt or truncated download, which is
the common failure; it does not catch a tampered release, because the sha
travels alongside the file. If you really cannot verify and you accept the
risk, `--no-verify` skips it.

With `--connect` it also registers it in your agents: in Claude Code with
`claude mcp add --scope user`, and in opencode by editing its configuration
(or creating it, if opencode is installed and does not have one yet). If it
cannot do either of those two things on its own, it says so and prints the
exact line.

The only thing left to do by hand is the Claude Code **plugin**, if you want
it: those two commands are typed inside Claude Code and no script can type
them. The MCP server, which is what matters, does get connected.

The rest of this document explains each step separately, in case you prefer
doing it by hand or something fails.

## 1. Installing the binary by hand

### Downloading the prebuilt one

Pick the file for your platform in
[the releases](https://gitlab.com/__alexander__/anathem/-/releases) —
`mem-linux-x86_64.tar.gz`, `mem-macos-arm64.tar.gz`, `mem-macos-x86_64.tar.gz`
or `mem-windows-x86_64.zip` — and unpack it:

```bash
tar -xzf mem-macos-arm64.tar.gz
mkdir -p ~/.local/bin && mv mem ~/.local/bin/mem && chmod 755 ~/.local/bin/mem
mem --version
```

Each file comes with its `.sha256` next to it. To check it:

```bash
shasum -a 256 -c mem-macos-arm64.tar.gz.sha256   # macOS
sha256sum -c mem-macos-x86_64.tar.gz.sha256      # Linux
```

**Both operating systems will warn you about this binary, and they are right
to.** It is unsigned, and nobody has downloaded it enough times for it to have
a reputation. That describes every small project, so the warning says nothing
about whether the file is sound — but it is the first thing a new person
meets, so it is better said here than discovered.

- **macOS**: Gatekeeper reports that the developer cannot be verified, because
  the binary is not signed with an Apple developer account and not notarized.
  Clear it with `xattr -d com.apple.quarantine ~/.local/bin/mem`, or from
  Settings → Privacy and Security, which offers to open it anyway.
- **Windows**: SmartScreen shows "Windows protected your PC" for the same
  reason — no Authenticode signature and no reputation. *More info* → *Run
  anyway*.

**Downloading with `curl` avoids both.** The marker that triggers them —
`Mark of the Web` on Windows, `com.apple.quarantine` on macOS — is attached by
browsers and mail clients, not by `curl`, which is what the installer uses.
If you fetch the release by hand from a browser, you meet the warning; if you
run the one-line install, you should not.

What replaces a signature here is one you can verify yourself: each release
carries a `.sha256` beside every file and a
[minisign](https://jedisct1.github.io/minisign/) `.minisig` made with the key
in [`anathem.pub`](../anathem.pub).

```bash
minisign -Vm mem-macos-arm64.tar.gz -p anathem.pub
```

That proves the file is the one published here, by whoever holds that key. It
tells your operating system nothing, and it is not a substitute for signing —
it is what this project can honestly offer.

### Compiling from source

```bash
git clone https://gitlab.com/__alexander__/anathem.git anathem
cd anathem
cargo build --release
mkdir -p ~/.local/bin && cp target/release/mem ~/.local/bin/mem && chmod 755 ~/.local/bin/mem
```

(`mkdir`+`cp`+`chmod` is used rather than `install -D` because that flag is
from GNU coreutils and does not exist in the `install` of macOS or the BSDs.)

Make sure `~/.local/bin` is on your `PATH`. The binary weighs about 10 MB and
depends on nothing installed on the system: it compiles SQLite from source
(`rusqlite` with `bundled`) and carries the tokenizer inside.

**A C toolchain is needed** for that compilation. Depending on the system:

| system | what is needed |
|---|---|
| Linux | `gcc` or `clang`, which is usually there; if not, the `build-essential` package or its equivalent |
| macOS | the Xcode command line tools: `xcode-select --install` |
| Windows | the *Build Tools for Visual Studio* with the C++ workload — see [`windows.md`](windows.md) |

### macOS, specifically

It works the same as on Linux — same POSIX semantics — and needs no code
changes at all. Three things that do differ:

1. **`~/.local/bin` is not on the `PATH` by default.** Add it to your
   `~/.zshrc` (zsh is the default shell since Catalina):
   `export PATH="$HOME/.local/bin:$PATH"`. The Claude Code plugin invokes the
   binary as `mem`, so without this it does not start.
2. **Xcode Command Line Tools**, as the table says. If `cargo build` fails in
   `libsqlite3-sys` or in `onig_sys`, that is it and not anathem.
3. **Apple Silicon needs nothing special**: Rust compiles to
   `aarch64-apple-darwin` by default, and so do the C dependencies.

One test is skipped on macOS — the one that measures resident memory, because
it reads `/proc`, which does not exist there — and it says so when it runs.
The rest of the suite runs in full.

**Tested on macOS**, ever since there is CI: every push compiles on a
`macos-14` runner (Apple Silicon) and runs the full suite, plus a run
through `init`, `save`, `search` and `doctor`. What CI does not cover is
the installation itself — Gatekeeper, your shell's `PATH`, the Claude Code
plugin — so if something fails there, that is valuable information.

**There is no service to bring up.** anathem is not a daemon: it does not
listen on any port, there is nothing to start when the machine boots, and
after a reboot nothing is left running. All the state is files. The agent —
Claude Code, opencode — launches `mem serve` as a subprocess when it starts
and terminates it when it closes.

## 2. Creating the database and importing a corpus (optional)

```bash
mem --db ~/.anathem/mem.db init
```

If you are coming from supermemory (or from any source you can dump into the
same JSONL format, see `tools/export_supermemory.py` as a reference), migrate
the frozen export:

```bash
mem --db ~/.anathem/mem.db migrate --from /path/to/your-export.jsonl
```

`migrate` is idempotent: running it again over the same database does not
duplicate memories (it uses `ext_id` as the key). The command reports three
counts:

```
migrated=<n> archived=<n> failed=<n>
curated ones with status failed (not migrated, reimport by hand):
  <ext_id>  <title>
```

A curated document that the source export marked as `status: failed` is not
migrated automatically — its content stays intact in the JSONL. To reimport it
by hand:

```bash
mem --db ~/.anathem/mem.db save "<paste the content of that memory from the JSONL>" \
  --project <project>
```

If you have no corpus to migrate, `mem init` is enough to get going with an
empty database and start saving new memories with `mem save` or with
`save_memory` from the MCP server.

## 3. Installing the vocabulary pack (optional)

Without this, anathem searches with BM25 only and behaves as it always does.
The pack adds cross-language search: finding a memory written in Spanish by
asking in English, or the other way round. On a real corpus that takes
cross-language recall@5 from 34 % to 58 %, without losing anything on
same-language searches.

**The installer downloads it for you**, from the release, with its `.sha256`
and its signature beside it — so if you installed with `install.sh` and did
not pass `--no-pack`, you already have it and this section is only here to
explain what it is. It is a 22.5 MB binary file, kept out of version control
and published as a release asset instead.

To get it on its own there are three ways:

- **Download it from the [release](https://gitlab.com/__alexander__/anathem/-/releases)**,
  together with the `.sha256` next to it. `model install` wants both.
- **Have someone send it to you.** It is a loose file: sending it by any means
  works. It does not depend on the operating system or the architecture — it
  is a table of numbers, the same on Linux, macOS or Windows.
- **Generate it yourself**, with `tools/build_pack.py`, which is what you want
  for languages the published pack does not cover. That requires downloading
  the 512 MB base model and a text sample in the languages you want. The full
  procedure is in [`vocabulary-packs.md`](vocabulary-packs.md).

With the file in hand — and with its `.sha256` beside it, which is how both
the release and `build_pack.py` hand it over; `model install` refuses a pack
it cannot check, unless you pass `--no-verify`:

```bash
mem --db ~/.anathem/mem.db model install en-es.anathem-pack
mem --db ~/.anathem/mem.db model status
```

`status` has to say `coverage: N/N chunks`. If there are gaps, run
`mem reindex`. To go back, `mem model remove` deletes the vectors and **also
the pack file**, and search goes back to being purely lexical. Watch out for
that: if the `.anathem-pack` you installed was your only copy, `remove`
destroys it and reinstalling means downloading it again. Keep the file outside
`~/.anathem/packs/` if you want to be able to go back and forth. (An
installation made before this was renamed still has `paquetes/`, and keeps
working; `mem doctor` says how to move it.)

The pack weighs 22.5 MB and is the static model trimmed down to the languages
it covers. To generate one with other languages, see
[`vocabulary-packs.md`](vocabulary-packs.md).

## 4. The project detects itself

Every memory belongs to a project. When saving without `--project`, it is
derived from the repository you are standing in: the name of the `origin`
remote, or the name of the repo's folder if there is no remote, or
`no-project` outside any repository.

The remote is used rather than the directory on purpose: a repo usually lives
in a folder that is not named after it, and inside a worktree the folder is
named after the branch. The MCP server inherits the directory of the agent
that launched it, so in practice every Claude Code session saves into the
right project without anyone configuring anything.

    $ mem save "a note"
    saved memory 42 in api-monolito (detected)

The `(detected)` marker distinguishes what was derived from what was chosen by
hand.

The same detection decides what `search_memory` searches: the project the
session is in, so what was saved here is what is found here. The agent can
pass `all_projects: true` to look past it, and when the project comes back
empty the search widens by itself and says so, so nothing is ever out of
reach. On the command line `mem search` is unchanged: it searches everything
until you pass `--project`.

## 5. Checking resource consumption

The server **does not load the pack at startup**, but the first time somebody
searches or saves. A session that works on code and never queries the memory
pays nothing. Measured:

| moment | RSS |
|---|---|
| server started, no pack installed | 7 MB |
| server started, pack installed | 7 MB |
| after `initialize` and `tools/list` | 7 MB |
| after the first search | 64 MB |

Those 57 MB of the jump are almost all the **tokenizer**, not the embeddings
table: the 22.5 MB file is mapped and only about 19 MB stay resident — and
those are shared between processes — whereas parsing the tokenizer's 76,363
pieces builds heap structures that are private to each process.

That is why lazy loading matters: with several agent sessions open, only the
ones that really use the memory pay for it.

## 6. Connecting it to Claude Code

The shortest way is to let the CLI do it, which is what
`install.sh --connect` does:

```bash
claude mcp add --scope user anathem -- ~/.local/bin/mem --db ~/.anathem/mem.db serve
```

`--scope user` leaves it available in all your sessions. If the `claude`
command is not on your `PATH` — which happens when you only have the desktop
application — it has to be done by hand, which is what follows.

Add an `mcpServers` entry to your Claude Code configuration — `~/.claude.json`
so it is available in all your sessions, or an `.mcp.json` at the root of a
particular project for per-project scope:

```json
{
  "mcpServers": {
    "anathem": {
      "command": "/path/to/your/home/.local/bin/mem",
      "args": ["--db", "/path/to/your/home/.anathem/mem.db", "serve"]
    }
  }
}
```

If the file already has other top-level keys (`env`, `permissions`, `hooks`,
and so on), add `mcpServers` as one more key of the same object — do not
replace the whole file.

This tells Claude Code to start `mem serve` as an MCP server over stdio,
pointed at your database. After saving the file, in a **new** Claude Code
session 5 tools should appear: `search_memory`, `save_memory`, `get_memory`,
`list_tags` and `recent_memories`. To verify, ask Claude to search for
something you have already saved and confirm that `search_memory` returns real
results.

## 7. Connecting it to opencode

opencode's MCP configuration file is `~/.config/opencode/opencode.jsonc`. If
it already has a top-level `"mcp"` key with other servers, add anathem's entry
next to the ones that are already there, without touching the rest of the
file:

```jsonc
{
  "mcp": {
    "anathem": {
      "type": "local",
      "command": ["/path/to/your/home/.local/bin/mem", "--db", "/path/to/your/home/.anathem/mem.db", "serve"],
      "enabled": true
    }
  }
}
```

It is the same binary in `serve` mode, but with opencode's syntax (`command`
is an array with the binary and its arguments, instead of separate
`command`/`args` as in Claude Code). Verify it the same way as in the previous
step: a new opencode session, ask it to search for something you have already
saved, confirm that it brings back real results and not a connection error.

## 8. If you were coming from supermemory (or another system) and decide to turn it off

That is your decision, not something anathem resolves on its own — the
previous system may be something you depend on daily. Turning it off only
makes sense after confirming, with real use, that your agents are answering
well with anathem (steps 4 and 5 above, verified with new sessions of each
one).

**Do not delete anything up front.** Leave the previous system's data where it
is — export, blobs, backups — for a reasonable stretch of real anathem use
before considering deleting anything. If something in the migration turned out
to be incomplete, that data is still the only complete copy.

## 8.5. Several agents over the same memory

If you use more than one agent — Claude Code and opencode, for example — both
can point at the same database. There is nothing to synchronize: it is a
SQLite file in WAL mode, and both processes open it at the same time without
stepping on each other.

What anathem does on top of that is tell each agent, on connecting, what the
other left behind — without either of them having to ask for it. From the
terminal, the same thing:

```bash
mem recent --since 6h      # this project
mem recent --all           # all of them
```

That is the whole of the installation side. How the greeting works, which
clients honour it (Claude Code and opencode, verified; Cursor and Codex
unverified), what it costs per session, how to turn it down or off, and the
convention that makes it worth having — closing with `mem save --topic` so
each note replaces the last instead of piling up — are in
[`handoff.md`](handoff.md).

## 9. Final verification

Before calling the installation done:

```bash
cargo test                       # green
mem --db ~/.anathem/mem.db export --out /tmp/anathem-export
mem --db ~/.anathem/mem.db reindex
```

`export` should produce readable markdown with frontmatter, one file per
memory — `active`, `superseded`, `draft` and `forgotten` alike, since `forget`
hides rather than deletes — organized by project.
`reindex` should rebuild chunks and the FTS5 index from `memories.body`
without errors and without changing the memory count.
