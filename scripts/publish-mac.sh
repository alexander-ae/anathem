#!/bin/sh
# Uploads the macOS binaries to a GitLab release.
#
# anathem lives on GitLab, but compiling for macOS requires Apple hardware and
# GitLab's macOS runners are Premium and Ultimate only. So that part is done by
# a private repository on GitHub, and this script is the bridge: it triggers
# the workflow, waits, downloads the artifacts and uploads them to the GitLab
# release.
#
# The bridge is a script and not a token stored on GitHub on purpose: that way
# there are no GitLab credentials on GitHub's side, and every publication is
# made by a person knowingly.
#
# It needs `gh` authenticated on the account that owns the private repository,
# and `glab` authenticated on GitLab.
set -eu

ETIQUETA="${1:-}"
REPO_GH="${ANATHEM_REPO_GH:-alexander-ae/anathem}"
REPO_GL="${ANATHEM_REPO:-__alexander__/anathem}"

if [ -z "$ETIQUETA" ]; then
  echo "Usage: sh scripts/publish-mac.sh <tag>    (for example v0.1.0)" >&2
  echo >&2
  echo "Variables: ANATHEM_REPO_GH (where it is built), ANATHEM_REPO (where it is published)" >&2
  exit 2
fi

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
malo() { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso() { printf '\n\033[1m%s\033[0m\n' "$1"; }

for cmd in gh glab; do
  command -v "$cmd" >/dev/null 2>&1 || { malo "$cmd is missing from the PATH"; exit 1; }
done

paso "1. Check that the release exists on GitLab"
# Uploading to a nonexistent release would create a loose, pointless one, so it
# is checked beforehand instead of discovered at the end.
if ! glab release view "$ETIQUETA" --repo "$REPO_GL" >/dev/null 2>&1; then
  malo "release $ETIQUETA does not exist in $REPO_GL."
  echo "    Create it first: the tag pipeline makes it, or" >&2
  echo "    glab release create $ETIQUETA --repo $REPO_GL" >&2
  exit 1
fi
ok "release $ETIQUETA found"

paso "2. Build on macOS"
gh workflow run release.yml --repo "$REPO_GH" -f etiqueta="$ETIQUETA"
ok "workflow triggered"

# `gh run watch` needs the id, and the run just triggered takes a while to show
# up.
CORRIDA=""
INTENTO=0
while [ -z "$CORRIDA" ] && [ "$INTENTO" -lt 30 ]; do
  sleep 2
  CORRIDA="$(gh run list --repo "$REPO_GH" --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)"
  INTENTO=$((INTENTO + 1))
done
[ -n "$CORRIDA" ] || { malo "could not find the run just triggered"; exit 1; }
ok "run $CORRIDA"

if ! gh run watch "$CORRIDA" --repo "$REPO_GH" --exit-status --interval 20; then
  malo "the build failed; see gh run view $CORRIDA --repo $REPO_GH --log-failed"
  exit 1
fi
ok "build finished"

paso "3. Download the artifacts"
TEMPORAL="$(mktemp -d)"
trap 'rm -rf "$TEMPORAL"' EXIT INT TERM
gh run download "$CORRIDA" --repo "$REPO_GH" --dir "$TEMPORAL"
# `gh run download` makes one directory per artifact; we put them together.
find "$TEMPORAL" -type f -name 'mem-macos-*' -exec mv {} "$TEMPORAL"/ \; 2>/dev/null || true
CUANTOS="$(find "$TEMPORAL" -maxdepth 1 -type f -name 'mem-macos-*.tar.gz' | wc -l)"
[ "$CUANTOS" -gt 0 ] || { malo "no macOS binary was downloaded"; exit 1; }
ok "$CUANTOS macOS binaries"

paso "4. Verify before uploading"
for t in "$TEMPORAL"/mem-macos-*.tar.gz; do
  s="$t.sha256"
  [ -f "$s" ] || { malo "the sha256 of $(basename "$t") is missing"; exit 1; }
  ( cd "$TEMPORAL" && shasum -a 256 -c "$(basename "$s")" >/dev/null 2>&1 \
    || sha256sum -c "$(basename "$s")" >/dev/null 2>&1 ) \
    || { malo "the sha256 of $(basename "$t") does not match"; exit 1; }
  ok "$(basename "$t"): sha256 correct"
done

paso "5. Upload to the GitLab release"
for f in "$TEMPORAL"/mem-macos-*; do
  glab release upload "$ETIQUETA" "$f" --repo "$REPO_GL"
  ok "uploaded: $(basename "$f")"
done

paso "Done."
echo "  https://gitlab.com/$REPO_GL/-/releases/$ETIQUETA"
