# Security: what can go wrong, and what anathem does about it

anathem stores text and then puts it into a model's context. That is, by
construction, a prompt injection channel: the day a memory contains
instructions instead of facts, the agent is going to read them.

This document says what the real risk is, what is mitigated, what is **not**,
and what the person using it can do. What it does not do is promise that the
problem is solved, because it is not solved anywhere: there is no reliable way
to separate data from instructions when both are text that the model reads.

## What helps before we start: there is no outbound channel

The most dangerous scenario the literature describes is the combination of
three things: private data, untrusted content, and a channel to send
information out. With all three, an injection can exfiltrate.

anathem has the first two and **not the third**. It does not talk to any
server while it works: no telemetry, no synchronization, no API. The binary's
only network connection is `mem update`, which a person asks for and which
only downloads a version number.

That does not make an injection harmless — it can make the agent lie to you,
delete something of yours, or suggest a malicious change — but it does take
away anathem's role as the way out. The agent may have its own channels, and
that is outside what anathem controls.

## Where text you did not write comes in

| route | who writes it | trust |
|---|---|---|
| `mem save` in the terminal | you | high: you typed it |
| `save_memory` from the agent | the agent, summarizing what it read | **low**: it can come from a page, a ticket, a PR comment, a file in the repo |
| `mem migrate` | an export from another system | whatever that system had |
| `mem import` | files from a folder | **low if the folder is not yours** |

The row that matters is the second one. An agent that reads a web page and
saves a summary can save, without knowing it, the text that page put there so
that it would be saved.

## Where it goes out towards the model

1. **The handoff in `initialize`.** This is the most sensitive one: it goes
   into the system prompt, in a position of trust, in **every** session of
   that project. See `recent::handoff_instructions`.
2. **`search_memory` results** and the body that `get_memory` returns. They
   come in as a tool result, which is a position of less trust, but the model
   reads them all the same.

## What is mitigated, and with what degree of certainty

**Reliable — the content cannot break the structure.** The handoff goes inside
a delimited block, and everything that enters it passes through a sanitizer
that changes `<` and `>` for their typographic variants, collapses newlines
and trims. A memory containing the closing delimiter comes out with the signs
changed: it is visible, it has no effect, and the block stays well formed.
There are tests that pin this down (`a_memory_cannot_close_the_data_block`).

This is reliable because it does not depend on guessing intentions: it is a
structural property, it either holds or it does not, and a test checks it.

**Reliable — a memory cannot take over the prompt.** Each title is trimmed and
the number of memories in the handoff is bounded by `handoff_items`. Without
that, a single long note could push the rest of the prompt out.

**Partial — the block is labelled.** The text says explicitly that what is
inside it is data, that it may come from pages or tickets, and that any
directions appearing there are not to be followed. It also says, for each
note, **who saved it**: the agent's name, or "written in the terminal" if you
typed it yourself. A well aligned model uses that signal.

It is partial because it depends on the model, not on a guarantee. It helps;
it does not close the hole.

**Not attempted — filtering instructions out of the text.** There is no list
of forbidden phrases, no detection of "ignore the above". That is deliberate:
that kind of filtering does not work, and believing that it does is worse than
not having it, because it invites you to trust memories that passed the
filter.

## What is NOT mitigated

- **A memory with plausible instructions.** If a note says "the team's
  convention is to disable certificate verification", the agent may believe
  it. It is indistinguishable from a legitimate memory that happens to be
  wrong.
- **The full body that `get_memory` returns.** It goes out unsanitized, on
  purpose: it is the content you asked for, and trimming it would make it
  useless.
- **`mem import` of somebody else's folder.** It imports whatever is there. It
  is the equivalent of opening a repository that is not yours.
- **What the agent does with the injection.** anathem does not control the
  agent's other tools.

## What you can do

- **Look at what comes in.** `mem recent` shows the latest along with who
  saved it. Anything carrying an agent's name was written by a model, not by
  you.
- **`mem forget <id>`** takes a memory out of search without deleting it, and
  `mem supersede` replaces it. Both are reversible.
- **Lower or turn off the handoff** if you do not want it in the system
  prompt: `"handoff_items": 0` in `config.json`. The `recent_memories` tool
  stays available for when the agent asks for it.
- **Do not import other people's folders** without reading them, just as you
  would not run a script somebody handed you.

## The rest of the surface

- **The vocabulary pack** is verified by sha256 on install, and the format is
  validated on open: magic, version, dimensions and sizes, with bounded reads.
  A corrupt pack makes the open fail, not the search: the vector layer turns
  off and bm25 remains.

  The **name** the pack declares is validated too, and that is not a detail:
  that name decides the path of the installed file
  (`paquetes/<name>.anathem-pack`), so one with `..` or an absolute path would
  write wherever it wanted with whatever content it wanted. It has to be a
  simple file name or the pack does not open.

  Once installed it is left read-only. Not because it is secret — the pack is
  a public download — but because it is mapped into memory for as long as the
  server lives, and truncating a mapped file takes the process down with
  SIGBUS. Reinstalling still works: it replaces the file with a `rename`,
  which asks for permission on the directory and not on the file.

  What the sha256 does **not** give is authenticity: it comes from the same
  release as the pack, so it detects a corrupt download and not a tampered
  release. Whoever could replace one could replace the other.

  Closing that needs a signature checked against a key published somewhere
  else, and the plumbing for it is here: `scripts/sign-release.sh` signs a
  published release with minisign and `sh install.sh --verify-signature`
  checks it. The key is held by the maintainer and not by any CI system — a
  key in a pipeline signs whatever that pipeline produced, which is what the
  `.sha256` already attests, and this project builds on two systems, so such
  a key would have to exist in both places to cover everything. Until a
  release is actually signed, `--verify-signature` says so and refuses rather
  than pretending to check.

  What did change already is that nothing carries on when it cannot verify. The installer
  stops if the `.sha256` is missing or the machine has nothing to compute it
  with, and `mem model install` refuses a pack that arrives without its
  sidecar — every pack the builder writes and every pack a release publishes
  carries one, so its absence means the file travelled without it. To skip
  either you have to ask, with `--no-verify`.
- **`config.json` and `tags.json`** are yours and local; if they are broken,
  anathem fails loudly instead of carrying on with values you did not ask for.
- **The database** is an unencrypted SQLite file. Whoever can read your disk
  can read your memories; anathem adds no protection there. What it does do is
  not leave them readable for **other users of the same machine**: the
  database, its `-wal`/`-shm` files and the directory containing them are set
  to 0600 and 0700 every time they are opened, not only when they are created
  — an old database created as 0644 fixes itself. The same goes for the
  temporary file `mem edit` uses while you have the editor open, which lives
  in a private directory and disappears with it even if the editor fails.
- **A memory received from another machine** (`mem save --external`) travels
  over SSH, so it opens no new door: whoever can run `mem save` on a host can
  already run anything there. What it does change is the chain of custody. A
  received memory is text another model wrote, perhaps while summarising a
  page it read, and you never saw the source.

  So it arrives as a **draft**, which every search filters out, and stays
  there until a person runs `mem accept <id>`. Nothing promotes one on its
  own. `mem recent --external` shows what is waiting, `mem show <id>` reads
  one before you decide, and `mem doctor` says how many there are. Once
  accepted it carries `[from: sender]` wherever it appears, in `mem search`
  and in what the MCP server injects, so an agent can tell what this
  installation was told from what it observed.

  anathem grows no port, no daemon and no API key for this. SSH already
  solves authentication, encryption, key management and auditing, and a
  listening process would replace "the access control is the filesystem"
  with a surface that exists while you sleep.
- **The search log** records what the MCP server was asked, what came back
  and whether the agent then opened one of the results. It is your own
  activity, it never leaves the machine — there is no outbound channel, see
  the first section — and it lives in the same database, with the same
  permissions. Nothing trims it: it grows for as long as you use the server,
  `mem doctor` says how large it has got, and `mem log --prune <window>`
  removes what is older than a window you give it. There is no default
  window and nothing deletes rows on its own, because retrieval history is
  the kind of thing that should go when a person says so.
- **The binary** does not update itself and does not execute anything that
  comes from the database.

## If you find something

Open an issue describing the concrete path. What helps most is a reproducible
case: what gets saved, what the agent receives, and what it does with that.
