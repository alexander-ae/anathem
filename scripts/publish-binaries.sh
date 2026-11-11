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

EXTRA=""
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
# An asset that is already there is left alone rather than retried: GitLab
# answers a duplicate link with a 400 that aborts the whole upload, so one
# file already published used to take the other three down with it. That
# happens whenever a release was started another way — v0.3.0 had its Linux
# and Windows halves published by the pipeline before the builds moved here —
# and it has to be survivable, because a publish script that cannot be run
# twice is one you cannot resume when the network drops halfway.
YA="$(glab release view "$ETIQUETA" --repo "$REPO_GL" 2>/dev/null | awk '/^[a-zA-Z0-9._-]+\t/ {print $1}')"
SUBIDOS=0
for f in $PRESENTES; do
  if printf '%s\n' "$YA" | grep -qx "$f"; then
    aviso "$f is already in the release; leaving it as it is"
    continue
  fi
  glab release upload "$ETIQUETA" "$TEMPORAL/$f" "$TEMPORAL/$f.sha256" --repo "$REPO_GL"
  ok "uploaded $f and its sha256"
  SUBIDOS=$((SUBIDOS + 1))
done
[ "$SUBIDOS" -gt 0 ] || aviso "nothing new to upload; checking what is published anyway"

paso "5. The vocabulary pack"

# The installer downloads the pack from the SAME release as the binary, and
# defaults to `permalink/latest`. A release published without it therefore
# degrades every new install to lexical search, quietly: the installer warns
# and carries on, which is the right thing for it to do and the wrong thing
# to rely on.
#
# This happened. v0.4.0 went out with four binaries and no pack, because this
# script only ever dealt with what the build produced and the pack is not
# built here — `scripts/publish.sh` carried it over and this one did not. The
# two publishing paths disagreed, and the one in use was the one that forgot.
PAQUETE=en-es.anathem-pack
YA="$(glab release view "$ETIQUETA" --repo "$REPO_GL" 2>/dev/null | awk '/^[a-zA-Z0-9._-]+\t/ {print $1}')"
if printf '%s\n' "$YA" | grep -qx "$PAQUETE"; then
  ok "already in the release"
else
  # The newest release that is not this one. `per_page=2` and not 1, because
  # this release exists by now and would be its own source.
  ANTERIOR="$(glab api "projects/$(printf '%s' "$REPO_GL" | sed 's|/|%2F|')/releases?per_page=2" 2>/dev/null \
    | tr ',' '\n' | sed -n 's/.*"tag_name":"\([^"]*\)".*/\1/p' | grep -vx "$ETIQUETA" | head -1)"
  if [ -z "$ANTERIOR" ]; then
    malo "no earlier release to take $PAQUETE from, and this one has none"
    echo "      Every install would fall back to bm25. Build it with" >&2
    echo "      tools/build_pack.py and upload it before announcing this." >&2
    exit 1
  fi
  ok "taking it from $ANTERIOR"
  LLEVADOS=""
  for f in "$PAQUETE" "$PAQUETE.sha256" "$PAQUETE.minisig"; do
    URL="https://gitlab.com/$REPO_GL/-/releases/$ANTERIOR/downloads/$f"
    if curl -fsSL --retry 3 --connect-timeout 20 -o "$TEMPORAL/$f" "$URL"; then
      LLEVADOS="$LLEVADOS $f"
      ok "downloaded $f"
    elif [ "$f" = "$PAQUETE" ]; then
      malo "$ANTERIOR has no $PAQUETE either"
      exit 1
    else
      # The signature is only published for releases that were signed, so
      # its absence is not a failure. The pack's absence is.
      aviso "$ANTERIOR has no $f"
    fi
  done
  # The sha travels with it, so checking costs nothing and the thing being
  # republished is exactly what was published.
  if [ -f "$TEMPORAL/$PAQUETE.sha256" ]; then
    ( cd "$TEMPORAL" && { sha256sum -c "$PAQUETE.sha256" >/dev/null 2>&1 \
        || shasum -a 256 -c "$PAQUETE.sha256" >/dev/null 2>&1; } ) \
      && ok "sha256 verified here" \
      || { malo "the carried-over pack does not match its sha256"; exit 1; }
  else
    malo "$ANTERIOR published $PAQUETE with no sha256; not republishing it unchecked"
    exit 1
  fi
  for f in $LLEVADOS; do
    glab release upload "$ETIQUETA" "$TEMPORAL/$f" --repo "$REPO_GL"
  done
  ok "carried over from $ANTERIOR"
  # Not into `PRESENTES`: that list holds base names and the check below
  # appends `.sha256` to each. These are already whole file names.
  EXTRA="$EXTRA $LLEVADOS"
fi

paso "6. Download every link back"

ROTOS=0
SINCOMPROBAR=0
# The binaries, each with the sha256 published beside it, plus whatever the
# pack step carried over under its own whole name.
LISTA=""
for f in $PRESENTES; do LISTA="$LISTA $f $f.sha256"; done
LISTA="$LISTA $EXTRA"
# A 429 is not a broken link, and this loop runs right after uploading tens
# of megabytes to the same host — which is exactly when GitLab starts
# answering 429 to downloads from this IP. Counting those as broken would
# end a correct publish with "the release is not usable", and `sign-release.sh`
# carries the same distinction for the same reason.
for archivo in $LISTA; do
  URL="https://gitlab.com/$REPO_GL/-/releases/$ETIQUETA/downloads/$archivo"
  INTENTO=1
  while [ "$INTENTO" -le 4 ]; do
    CODIGO="$(curl -s -o /dev/null -w '%{http_code}' -L "$URL" || echo 000)"
    case "$CODIGO" in
      429|5??|000) [ "$INTENTO" -lt 4 ] && sleep $((INTENTO * 4)) ;;
      *) break ;;
    esac
    INTENTO=$((INTENTO + 1))
  done
  case "$CODIGO" in
    200)
      ok "$archivo" ;;
    429|5??|000)
      aviso "$archivo -> HTTP $CODIGO, so it could not be checked"
      SINCOMPROBAR=$((SINCOMPROBAR + 1)) ;;
    *)
      malo "$archivo -> HTTP $CODIGO"
      echo "      $URL" >&2
      ROTOS=$((ROTOS + 1)) ;;
  esac
done
[ "$ROTOS" -eq 0 ] || { malo "$ROTOS links do not work; the release is not usable"; exit 1; }
if [ "$SINCOMPROBAR" -gt 0 ]; then
  aviso "$SINCOMPROBAR links could not be checked; nothing says they are broken"
  echo "      GitLab rate-limits downloads from this IP, and this script has" >&2
  echo "      just spent that budget uploading. Wait and run it again: the" >&2
  echo "      assets already there are left alone and only the check runs." >&2
  exit 1
fi

paso "Done"
echo "  The vocabulary pack is carried over separately; see scripts/publish.sh."
echo "  Sign the release with: sh scripts/sign-release.sh $ETIQUETA"
