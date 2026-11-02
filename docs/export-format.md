# The export format

`mem export --out folder/` writes one Markdown file per memory, with YAML
frontmatter, grouped into subfolders by project:

    folder/
      api-monolito/
        2026-08-11-the-public-api-rate-limit-is-170-per-minute-42.md
      personal/
        2026-09-23-prefers-commit-messages-in-the-imperative-91.md

The file name is `{created}-{slug of the title}-{id}.md`. The id suffix is
there because the slug is cut at eight words, and two memories from the same
project, with the same date and a similar long title, produced the same name —
it happened on a real corpus.

## The export is complete

What comes out is enough to rebuild the database in another tool. **The only
thing that does not travel is the vectors**, and those are recomputed with
`mem model install` plus `mem reindex`: they are derived from the text, not
original data.

**The `forgotten` memories are exported too.** `forget` hides, it does not
delete; leaving them out would turn every migration into a silent loss of
data. They come with `status: forgotten` so whoever reads the export can
filter them.

## Fields

Always present:

| field | type | what it is |
|---|---|---|
| `id` | integer | primary key in the source database; what `supersedes` points at |
| `name` | text | slug of the title, for tools that index by name |
| `description` | text | the title, on one line |
| `metadata.type` | text | always `project`, for compatibility with frontmatter readers |
| `project` | text | the project it belongs to; also the subfolder |
| `tags` | list of text | assigned tags, in alphabetical order |
| `created` | date | when it was saved |
| `updated` | date | last edit |
| `status` | text | `active`, `superseded`, `draft` or `forgotten` |
| `source` | text | `explicit` (the user asked for it), `suggested` or `migrated` |
| `pinned` | boolean | whether it survives the candidate trimming in search |

Present only when they have a value — the key is omitted instead of emitting
`null`, because frontmatter without the key reads the same in any parser and
does not invite confusing "has none" with "has null":

| field | type | what it is |
|---|---|---|
| `ext_id` | text | identifier in the system it was migrated from |
| `supersedes` | integer | the `id` of the memory this one replaces |
| `topic` | text | the topic key it was saved with; the current one for each key is the only `active` memory among those sharing it in a project |
| `event_start` | date | start of the period it refers to |
| `event_end` | date | end of that period |
| `refs` | list of `{kind, value}` | tickets, PRs and commits extracted from the body; `kind` is `ticket`, `pr` or `sha` |

The body of the memory goes after the frontmatter, as is, untransformed.

## About the YAML

Every text value is emitted in double quotes and with escapes. This is not
cosmetic: without it, a title containing `": "` made the parser read it as a
nested key and the file stopped being valid YAML — it happened in 52 out of
170 files of a real corpus.

## The other way out

The export is convenient to read and to import into tools that understand
Markdown. But **the real complete way out is `~/.anathem/mem.db`**: plain
SQLite, an open and documented format, readable with any tool without going
through anathem. If the goal is not to get locked in, that file is the
guarantee; the export is the convenience.
