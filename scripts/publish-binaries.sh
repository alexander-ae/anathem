#!/bin/sh
# Builds every binary on GitHub Actions and puts them on the GitLab release.
#
# anathem lives on GitLab. The compiling happens on GitHub because Actions is
# free and unlimited for public repositories on all three operating systems,
# while GitLab's shared-runner quota is 400 minutes a month — one release used
# to spend 28 of them on Windows alone — and macOS runners are not available
# there at any price below Premium.
#
# Nothing on GitHub publishes anything. The workflow leaves artifacts and this
# script, run by a person on their machine, downloads them and uploads them
# here. That way no GitLab credential is stored on GitHub's side.
#
# It needs `gh` authenticated on the account that owns the mirror and `glab`
# authenticated on GitLab.
#
#     sh scripts/publish-binaries.sh v0.3.0
#
# What it does, in order, and why each step is there:
#
#   1. pushes the tag to the mirror — the workflow checks out that tag, and a
#      mirror without it builds nothing, or worse, the wrong thing;
#   2. runs the workflow and waits;
#   3. downloads the artifacts and hashes them here, rather than trusting a
#      hash computed on the machine that built them;
#   4. uploads binaries and hashes to the GitLab release;
#   5. downloads every link back and fails if any does not answer 200.
#
# Steps 1 and 5 exist because of two things that actually happened. The macOS
# binaries published as v0.2.1 and v0.2.2 were built from v0.2.0's code,
# because nothing synced the mirror and the workflow ignored the tag it was
# handed. And v0.2.2's Linux and Windows links were all 404 for a day, because
# registering a link is not publishing a file.
set -eu

ETIQUETA="${1:-}"
REPO_GH="${ANATHEM_REPO_GH:-alexander-ae/anathem}"
REPO_GL="${ANATHEM_REPO:-__alexander__/anathem}"

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
aviso() { printf '  \033[33m!\033[0m %s\n' "$1" >&2; }
malo()  { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

if [ -z "$ETIQUETA" ]; then
  echo "Usage: sh scripts/publish-binaries.sh <tag>    (for example v0.3.0)" >&2
  echo >&2
  echo "Variables: ANATHEM_REPO_GH (where it builds), ANATHEM_REPO (where it publishes)" >&2
  exit 2
fi

for cmd in gh glab git curl; do
  command -v "$cmd" >/dev/null 2>&1 || { malo "$cmd is missing from the PATH"; exit 1; }
done

ARCHIVOS="mem-linux-x86_64.tar.gz mem-windows-x86_64.zip mem-macos-arm64.tar.gz mem-macos-x86_64.tar.gz"

paso "1. Check the tag, here and there"

git rev-parse -q --verify "refs/tags/$ETIQUETA" >/dev/null \
  || { malo "there is no tag $ETIQUETA in this repository"; exit 1; }
ok "$ETIQUETA exists locally"

CUENTA="$(gh api user --jq .login 2>/dev/null || echo unknown)"
gh api "repos/$REPO_GH" >/dev/null 2>&1 || {
  malo "the active gh account ($CUENTA) cannot see $REPO_GH."
  echo "    Switch with \`gh auth switch\`, or set ANATHEM_REPO_GH." >&2
  exit 1
}
ok "gh sees $REPO_GH as $CUENTA"

# The step whose absence published v0.2.0's code as v0.2.2 on macOS.
REMOTO_GH="$(git remote -v | awk -v r="$REPO_GH" '$2 ~ r {print $1; exit}')"
if [ -z "$REMOTO_GH" ]; then
  malo "no git remote points at $REPO_GH; add one and run this again"
  exit 1
fi
git push "$REMOTO_GH" "$ETIQUETA" 2>/dev/null || true
ok "$ETIQUETA is on the mirror"

paso "2. Build on GitHub Actions"

gh workflow run release.yml --repo "$REPO_GH" --ref main -f "tag=$ETIQUETA"
ok "workflow dispatched"

# The run id is not returned by `workflow run`, so it is looked up. A couple
# of seconds pass before the run exists at all.
CORRIDA=""
INTENTO=0
while [ -z "$CORRIDA" ] && [ "$INTENTO" -lt 20 ]; do
  sleep 3
  CORRIDA="$(gh run list --repo "$REPO_GH" --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)"
  INTENTO=$((INTENTO + 1))
done
[ -n "$CORRIDA" ] || { malo "could not find the run that was just dispatched"; exit 1; }
ok "run $CORRIDA"

gh run watch "$CORRIDA" --repo "$REPO_GH" --exit-status || {
  malo "the build failed; see gh run view $CORRIDA --repo $REPO_GH --log-failed"
  exit 1
}
ok "every platform built, and each one checked it is $ETIQUETA"

paso "3. Download the artifacts and hash them here"

TEMPORAL="$(mktemp -d)"
trap 'rm -rf "$TEMPORAL"' EXIT
gh run download "$CORRIDA" --repo "$REPO_GH" --dir "$TEMPORAL"

PRESENTES=""
for f in $ARCHIVOS; do
  # `gh run download` puts each artifact in its own directory.
  encontrado="$(find "$TEMPORAL" -name "$f" -type f | head -1)"
  if [ -z "$encontrado" ]; then
    aviso "$f did not come back from the build"
    continue
  fi
  mv "$encontrado" "$TEMPORAL/$f"
  # Hashed here and not there: a hash computed by the machine that built the
  # file says nothing the file does not already say about itself.
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$TEMPORAL" && sha256sum "$f" > "$f.sha256")
  else
    (cd "$TEMPORAL" && shasum -a 256 "$f" > "$f.sha256")
  fi
  ok "$f"
  PRESENTES="$PRESENTES $f"
done
[ -n "$PRESENTES" ] || { malo "the build produced none of the expected files"; exit 1; }

paso "4. Upload to the GitLab release"

glab release view "$ETIQUETA" --repo "$REPO_GL" >/dev/null 2>&1 || {
  aviso "there is no release $ETIQUETA on GitLab yet; creating it"
  glab release create "$ETIQUETA" --repo "$REPO_GL" --notes "See CHANGELOG.md."
}
for f in $PRESENTES; do
  glab release upload "$ETIQUETA" "$TEMPORAL/$f" "$TEMPORAL/$f.sha256" --repo "$REPO_GL"
  ok "uploaded $f and its sha256"
done

paso "5. Download every link back"

ROTOS=0
for f in $PRESENTES; do
  for archivo in "$f" "$f.sha256"; do
    URL="https://gitlab.com/$REPO_GL/-/releases/$ETIQUETA/downloads/$archivo"
    CODIGO="$(curl -s -o /dev/null -w '%{http_code}' -L "$URL")"
    if [ "$CODIGO" = "200" ]; then
      ok "$archivo"
    else
      malo "$archivo -> HTTP $CODIGO"
      echo "      $URL" >&2
      ROTOS=$((ROTOS + 1))
    fi
  done
done
[ "$ROTOS" -eq 0 ] || { malo "$ROTOS links do not work; the release is not usable"; exit 1; }

paso "Done"
echo "  The vocabulary pack is carried over separately; see scripts/publish.sh."
echo "  Sign the release with: sh scripts/sign-release.sh $ETIQUETA"
