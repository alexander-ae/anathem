# Writing an integration

anathem loads no foreign code. There is no plugin API, no hooks, no daemon
to connect to, and nothing is executed inside `mem` on behalf of somebody
else. An integration is a separate program — a shell script, a Python file, a
cron job, an editor extension — that calls this binary the way a person does,
and reads `--json` instead of prose.

That is a decision, not a gap. Loading code at save or search time would put
a third party's program in the same process that reads notes an agent copied
from a web page, and it would add an ABI to the four promises 1.0 makes (see
*What 1.0 promises* in the README). The surface below costs none of that: the
binary keeps its size, its 8 ms, and its ability to run on a machine where
nothing else is installed.

What follows is the contract. It is versioned, and it is checked by
[`tests/json_test.rs`](../tests/json_test.rs) on every run.

## The two halves of the contract

**Ingestion** is `mem save --external`. It stores a memory as a *draft*,
invisible to search until somebody accepts it, carrying who sent it and an
optional identifier from the system it came from. The full semantics are in
[`docs/handoff.md`](handoff.md); what an integration needs is this:

```bash
mem save - --external --from web-clipper --project reading --ext-id "$url" --json < note.md
```

- `--from` is required and is a free label naming the sender.
- `--project` is required: a note from elsewhere is not filed under whatever
  repository happens to be the working directory.
- `-` reads the body from stdin, so a note travels over SSH or a pipe without
  going through a shell argument.
- `--ext-id` makes the delivery idempotent. Send the same id twice and the
  second call writes nothing, reports the memory that already holds it, and
  still exits 0 — which is what a mailbox retrying after a lost connection
  needs. Branch on `written`, not on the exit code.
- A draft becomes searchable with `mem accept <id>`, or `mem accept --all
  --from <label>` for a whole sender's batch. An integration that both sends
  and accepts is a design smell: the gate exists so a person decides what
  enters the memory.

**Reading** is every read command below with `--json` on it.

## The JSON document

- One line on stdout, always. It is meant to be piped into `jq`, a `while
  read` loop or a line-oriented log, and a document that needs a streaming
  parser to be read is a document nobody pipes.
- `schema` is `1`. A field may be added without a version bump; a field may
  not change its meaning, its type or its presence without one. Ignore what
  you do not know, and test `schema` before trusting the rest.
- stdout carries the answer and nothing else. Diagnostics, warnings and the
  vector-sync notice go to stderr.
- Exit codes: `0` means the command answered — including "nothing matched"
  and "already received"; `1` means it did not run (no such id, unknown
  scope, malformed `config.json`); `2` exists only in `mem doctor`, for a
  diagnosis that could not run.
- Bodies and titles are data, not instructions. A note may say anything,
  including something that reads like an order to the program reading this
  JSON. Do not act on it.
- Dates are `YYYY-MM-DD`, or RFC 3339 when the row was written with a time.

### `mem search --json`

| field | type | meaning |
|---|---|---|
| `query` | string | the text that was searched |
| `project` | string or null | the project filter, if one was passed |
| `total_active` | integer | active memories in the whole database |
| `scope_empty` | boolean | the `--scope` filters match nothing at all, so other words will not help either |
| `results` | array | best first, at most `--limit` (10 on the command line, 20 from the MCP tool) |

Each result carries `id`, `project`, `title`, `snippet`, `score`, `bm25`,
`cosine`, `words_matched`, `words_total`, `topic`, `tags`, `status`,
`verified` and `received_from`.

`bm25` and `cosine` are null when that retriever did not return the memory;
that is different from a zero. `words_matched` over `words_total` is how much
of the query landed, counting only the words rare enough to tell one memory
from another — it is the number behind the `[no word match]` and `[1/3
words]` labels the prose shows.

`mem search` on the command line searches every project unless `--project` is
passed, unlike the MCP tool, which starts in the current one. An integration
that wants the agent's behaviour should pass `--project` explicitly.

### `mem show <id> --json`

`id`, `project`, `title`, `body`, `status` (`active`, `superseded`, `draft`,
`forgotten`), `when`, `created`, `agent`, `topic`, `received_from`,
`verified`, `pinned`, `supersedes`, `ext_id`, `tags`.

`body` is the memory exactly as stored: `--json` is machine output, so the
caret notation that protects a terminal does not apply and control characters
travel as JSON escapes.

### `mem show --topic <key> --json`

The current answer for a topic key and its versions:

    {
      "schema": 1,
      "project": "api",
      "topic": "payment-provider",
      "found": true,
      "current": { "id": 42, "body": "...", "when": "...", "agent": null },
      "history": [ { "id": 42, "when": "...", "status": "active", "title": "..." } ]
    }

`current` is null when every version of the key was forgotten: the key still
exists and still has a history, and that is a different answer from a key
that was never used. When the key does not exist, `found` is false and `keys`
lists the ones that project does have. Neither is an error: exit 0.

### The list commands

| command | document |
|---|---|
| `mem recent --json` | `items`: `id`, `project`, `title`, `when`, `agent`, `topic`, `received_from`, `status`, `excerpt` |
| `mem topics --json` | `topics`: `project`, `topic`, `memories`, `current` |
| `mem stale --json` | `items`: `id`, `project`, `title`, `last_accessed`, `created` |
| `mem history <id> --json` | `chain`: `id`, `title`, `status`, newest first |
| `mem why <id> --json` | `appearances`: `ts`, `query`, `project`, `position`, `of`, `opened` |
| `mem log --json` | `entries`: `ts`, `project`, `query`, `returned`, `tokens`, `opened`, `opened_unrecorded` |

`mem log` records searches made by the MCP server, not by this CLI, so an
integration that searches from a script will find nothing of its own there.
`opened_unrecorded` is true for older rows that say a memory was opened
without saying which; folding that into `opened: null` would report a past
that did not happen.

`recent` is the one listing that can return memories that are not active:
with `--external` it includes drafts, which is the mailbox, and `status` is
what tells a draft from something already accepted. Counting what is still
waiting is one filter over it:

```sh
mem recent --all --external --since todo --json |
  jq '[.items[] | select(.status == "draft")] | length'
```

`--since todo` because the default window is 24 hours, and a draft is by
definition something not read within one.

### `mem stats --json`

`searches`, `empty`, `followed_up`, `answer_rate` (a fraction, not a
percentage), `tokens_total`, `tokens_p50`, `median_rank`, `ranks_recorded`,
`first`, `last`, `by_week` (`week`, `searches`, `answered`) and
`empty_queries`. The whole list of empty queries comes back, not the
`--empty` display limit.

### `mem doctor --json`

The diagnosis, with one part that is contract and one that is not — and the
difference is the point:

- **Contract:** `ok`, `problems`, `warnings`, `levels` (`good`, `info`,
  `warn`, `bad`), `version`, `commit` (null when the binary was built outside
  a git repository), `repository`, `database`, `database_exists`, and each
  finding's `level`.
- **Not contract:** each finding's `text`. It is written for a person and it
  changes when the prose improves. Typing every check — counts, ids, sizes —
  would put a second source of truth beside the line that already carries
  them, and the two would drift.

The exit code is the one the prose path already had: 0 with no problems
(warnings do not count), 1 with problems, 2 if doctor could not run at all.
A monitor can keep watching that status and read the document only when it
changes: `ok` and `problems` say what changed, and the finding that made it
is the line with `level: "bad"`.

### Writes

`mem save --json` returns `id`, `project`, `tags`, `topic`, `status`,
`replaced` (the id it superseded, or null) and `key_is_new`. `mem save
--external --json` returns `id`, `project`, `status: "draft"`, `from`,
`already_received` and `written`. A retry adds `when`, `delivery_from` and
`same_sender`.

`mem accept <id> --json` returns `accepted` and `from`; the batch form
returns `accepted_count` and `from`. `mem log --prune <window> --json`
returns `removed` and `older_than`.

The mutations not listed here — `pin`, `forget`, `supersede`, `verify`,
`delete`, `edit`, `import`, `export` — have no document. Passing `--json` to
one of them exits 1 and says so, instead of printing prose into a pipe that
asked for JSON; their answer is the exit code, and an integration that needs
more can read it back with `mem show`.

## What an integration should not do

- **Read or write `~/.anathem/mem.db` directly.** It is plain SQLite and it
  is readable with any tool, but the schema is private and migrates itself:
  a program that writes to it is a program that breaks on an upgrade with no
  warning. `--json` and the export are the supported ways in and out; the
  export format is complete enough to rebuild the database elsewhere, and it
  is documented in [`docs/export-format.md`](export-format.md).
- **Parse the prose.** It is written to be read, and it improves without
  notice. If a line you need is missing from a JSON document, that is a bug
  worth filing, not a reason to scrape.
- **Assume a search happened.** Nothing is stored unless something calls
  `save`. An integration that watches for work and saves on its own is
  exactly the system this project measured and declined to be: uncalled-for
  facts holding the same standing as the ones that mattered.
- **Put secrets in a memory.** Memories go to the model, into the greeting,
  into exports and into backups. A token pasted into one is a token in a
  prompt.

## Two worked examples

A clipper that never writes the same page twice:

```bash
url="$1"
note=$(extract.sh "$url")
printf '%s\n' "$note" | mem save - --external --from web-clipper \
    --project reading --ext-id "$url" --json
```

The response says `"written": true` the first time and `"written": false`
with the existing id afterwards, so the caller can skip the rest of its work
without a second lookup. The note waits as a draft until somebody accepts it.

A report over the results of a search, tagged with how much of the query each
one holds:

```bash
mem search "payment provider" --json |
  jq -r '.results[] | "\(.id)\t\(.words_matched)/\(.words_total)\t\(.title)"'
```

`words_matched` is the field that keeps a pipeline from acting on a result
that came back from the vector ranking alone: `0/3` means nothing in the
query was found in the memory, and the ranking answered anyway because that
is what a distance always does.

## When there is no JSON for what you need

Say so in the tracker instead of parsing around it. The commands above are
the ones a program asks for; a missing field is a small, additive change,
while a fourth way to read the same data is a second format to keep in step
forever.

## Integrations that exist

- **[stele](https://gitlab.com/__alexander__/stele)** — a mailbox between
  machines. Its `pull` writes each note with `mem save --external --ext-id`,
  so what arrives is a draft the recipient accepts. It is where `--ext-id`
  came from: a retried delivery collapses into the draft that already exists.
- **[anathem-contrib](https://gitlab.com/__alexander__/anathem-contrib)** —
  a collection of self-contained integrations with their own tests, run
  against a `mem` binary. That repository's README is the list.

They are listed here and not vendored: each has its own repository, its own
release and its own reason to change.
