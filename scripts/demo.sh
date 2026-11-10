#!/bin/sh
# Sets up a throwaway corpus and runs the sequence a 30-second recording
# shows. Nothing here touches your own database: it builds one in a
# temporary directory and deletes it at the end.
#
#     sh scripts/demo.sh                 # run it
#     asciinema rec demo.cast -c 'sh scripts/demo.sh'
#
# Why a script and not a list of commands in an issue: a demo that cannot be
# re-run is a demo nobody re-records when the output changes, and this one
# has already changed once — an earlier draft superseded a memory with a
# correction that dropped the words the query used, so the search that had
# just worked came back empty on the next slide. Running it is how that was
# found.
#
# It uses `mem` from the PATH; pass another one as the first argument.
set -e

MEM="${1:-mem}"
command -v "$MEM" >/dev/null 2>&1 || { echo "no \`$MEM\` on the PATH" >&2; exit 1; }
D="$(mktemp -d)"
trap 'rm -rf "$D"' EXIT INT TERM
DB="$D/mem.db"

# Paced so a reader can follow without pausing. Tune or remove.
PAUSA="${PAUSA:-1.4}"
say()  { printf '\n\033[2m# %s\033[0m\n' "$1"; sleep "$PAUSA"; }
# The first argument is what the viewer reads; the rest is what runs. Two
# strings because the readable form carries quotes a shell would eat.
run()  { printf '\033[1m$ %s\033[0m\n' "$1"; shift; sleep 0.4; "$MEM" --db "$DB" "$@"; sleep "$PAUSA"; }

say "an agent saves what it worked out, in its own words"
run 'mem save "PAY-18 took production down: the migration held a lock on users for 40s" --topic deploy-incident' \
    save "PAY-18 took production down: the migration held a lock on users for 40s" --topic deploy-incident

# Two more so the search has something to rank against.
# Invented, like everything else here. A demo that leaks a real
# colleague's name or a real employer is a demo that gets recorded once
# and regretted afterwards.
"$MEM" --db "$DB" save "the payments service retries 3 times with backoff, then drops to the dead letter queue" >/dev/null
"$MEM" --db "$DB" save "staging is restored from prod every Sunday at 03:00 UTC" >/dev/null

say "a week later, in a new session, asked a different way"
run 'mem search "what took production down"' search "what took production down" --limit 2

say "it turns out the cause was something else. corrections are explicit"
run 'mem save "PAY-18 took production down: it was the index rebuild, not the migration" --topic deploy-incident' \
    save "PAY-18 took production down: it was the index rebuild, not the migration" --topic deploy-incident

say "the same question now answers with the correction"
run 'mem search "what took production down"' search "what took production down" --limit 1

say "and the old version is not gone, it just stopped competing"
run 'mem show --topic deploy-incident' show --topic deploy-incident

say "no daemon, no network, no API key. one file, and it checks itself"
printf '\033[1m$ %s\033[0m\n' 'mem doctor'
sleep 0.4
"$MEM" --db "$DB" doctor | grep -E "memories:|FTS5|All good|No problems"
sleep "$PAUSA"
