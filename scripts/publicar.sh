#!/bin/sh
# Publishes a release from this machine, without the CI.
#
# The pipeline in `.gitlab-ci.yml` does this on every tag and does it better:
# it builds Linux and Windows on clean runners. This script exists for when
# that is not available — the shared-runner compute quota runs out, GitLab is
# down, or you want to cut a release while offline from CI.
#
# It is deliberately NOT a full replacement. It builds for THIS machine only,
# so a release made here carries one platform's binary. What is missing is
# printed at the end, with how to add it: `scripts/publicar-mac.sh` brings the
# macOS ones, and Windows needs a Windows machine or the pipeline.
#
# A release with one binary is not a broken release: the installer falls back
# to compiling from source on any platform it finds no binary for. It is worse
# for whoever installs it, and the script says so rather than pretending.
set -eu

ETIQUETA="${1:-}"
REPO="${ANATHEM_REPO:-__alexander__/anathem}"
SALTAR_PRUEBAS=0
SIN_PAQUETE=0

for arg in "$@"; do
  case "$arg" in
    --skip-tests) SALTAR_PRUEBAS=1 ;;
    --no-pack)    SIN_PAQUETE=1 ;;
  esac
done

if [ -z "$ETIQUETA" ] || [ "${ETIQUETA#-}" != "$ETIQUETA" ]; then
  cat >&2 <<'FIN'
Usage: sh scripts/publicar.sh <tag> [options]     (for example v0.2.1)

  --skip-tests   do not run the suite first (you had better have a reason)
  --no-pack      do not carry the vocabulary pack over from the previous release

Variables: ANATHEM_REPO

It builds for THIS machine only. What is missing is printed at the end.
FIN
  exit 2
fi

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
aviso() { printf '  \033[33m!\033[0m %s\n' "$1"; }
malo()  { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

TEMPORAL=""
limpiar() { [ -n "$TEMPORAL" ] && rm -rf "$TEMPORAL" || true; }
trap limpiar EXIT INT TERM

paso "1. Checks before touching anything"

command -v glab >/dev/null 2>&1 || { malo "glab is missing from the PATH"; exit 1; }
glab auth status >/dev/null 2>&1 || { malo "glab is not authenticated; run \`glab auth login\`"; exit 1; }
ok "glab authenticated"

# Publishing from a dirty tree means publishing something that is in no
# commit, which nobody can reproduce afterwards.
if [ -n "$(git status --porcelain)" ]; then
  malo "the working tree has uncommitted changes."
  echo "    A release has to be reproducible from a commit." >&2
  exit 1
fi
ok "clean working tree"

RAMA="$(git rev-parse --abbrev-ref HEAD)"
if [ "$RAMA" != "main" ]; then
  aviso "you are on $RAMA, not main — publishing it anyway"
fi

# Releasing a tag that already has a release would silently add assets to it,
# which is how you end up with two platforms' binaries from two different
# commits under the same version.
if glab release view "$ETIQUETA" --repo "$REPO" >/dev/null 2>&1; then
  malo "release $ETIQUETA already exists in $REPO."
  echo "    To add binaries to an existing one, use \`glab release upload\`." >&2
  exit 1
fi
ok "release $ETIQUETA does not exist yet"

# The tag has to match what Cargo says, or the binary reports a version that
# is not the one published, and `mem update` compares against it.
VERSION_CARGO="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
if [ "$ETIQUETA" != "v$VERSION_CARGO" ]; then
  malo "the tag is $ETIQUETA and Cargo.toml says $VERSION_CARGO."
  echo "    They have to agree: \`mem --version\` reports Cargo's, and" >&2
  echo "    \`mem update\` compares it against the published tag." >&2
  exit 1
fi
ok "tag and Cargo.toml agree on $VERSION_CARGO"

paso "2. The suite"
if [ "$SALTAR_PRUEBAS" -eq 1 ]; then
  aviso "skipped by request"
else
  cargo test --locked --quiet
  ok "suite in the green"
fi

paso "3. Build"
SISTEMA="$(uname -s)"
ARQUITECTURA="$(uname -m)"
case "$SISTEMA/$ARQUITECTURA" in
  Linux/x86_64)   PLATAFORMA="linux-x86_64" ;;
  Darwin/arm64)   PLATAFORMA="macos-arm64" ;;
  Darwin/x86_64)  PLATAFORMA="macos-x86_64" ;;
  *)              malo "no release is published for $SISTEMA/$ARQUITECTURA"; exit 1 ;;
esac
ok "platform: $SISTEMA/$ARQUITECTURA → $PLATAFORMA"

cargo build --release --locked --quiet
./target/release/mem --version
ok "binary built"

TEMPORAL="$(mktemp -d)"
ARCHIVO="mem-$PLATAFORMA.tar.gz"
tar -C target/release -czf "$TEMPORAL/$ARCHIVO" mem
if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$TEMPORAL" && sha256sum "$ARCHIVO" > "$ARCHIVO.sha256" )
else
  ( cd "$TEMPORAL" && shasum -a 256 "$ARCHIVO" > "$ARCHIVO.sha256" )
fi
ok "packaged: $ARCHIVO ($(wc -c < "$TEMPORAL/$ARCHIVO" | tr -d ' ') bytes)"

paso "4. The vocabulary pack"
# The installer downloads the pack from the SAME release as the binary, so a
# release without it leaves every install degraded to lexical search. The pack
# does not change between versions — it is built by
# `tools/construir_paquete.py` and weighs 22 MB — so it is carried over from
# the previous release instead of being rebuilt.
if [ "$SIN_PAQUETE" -eq 1 ]; then
  aviso "not carried over, by request: installs will fall back to bm25"
else
  ANTERIOR="$(glab release list --repo "$REPO" 2>/dev/null | awk 'NR>1 {print $1; exit}')"
  if [ -z "$ANTERIOR" ]; then
    aviso "no previous release to take the pack from; installs will fall back to bm25"
  else
    ok "taking it from $ANTERIOR"
    for f in en-es.anathem-pack en-es.anathem-pack.sha256; do
      URL="https://gitlab.com/$REPO/-/releases/$ANTERIOR/downloads/$f"
      if curl -fsSL --retry 3 --connect-timeout 20 -o "$TEMPORAL/$f" "$URL"; then
        ok "downloaded: $f"
      else
        aviso "could not download $f from $ANTERIOR"
        rm -f "$TEMPORAL/en-es.anathem-pack" "$TEMPORAL/en-es.anathem-pack.sha256"
        break
      fi
    done
    # The sha travels with it, so it costs nothing to check that what is
    # about to be republished is what was published.
    if [ -f "$TEMPORAL/en-es.anathem-pack.sha256" ]; then
      ( cd "$TEMPORAL" && { sha256sum -c en-es.anathem-pack.sha256 >/dev/null 2>&1 \
          || shasum -a 256 -c en-es.anathem-pack.sha256 >/dev/null 2>&1; } ) \
        && ok "pack sha256 verified" \
        || { malo "the carried-over pack does not match its sha256"; exit 1; }
    fi
  fi
fi

paso "5. Publish"
# The tag is created by the release itself, from the current commit: one less
# step that can be forgotten, and no window where the tag exists without a
# release.
glab release create "$ETIQUETA" \
  --repo "$REPO" \
  --ref "$(git rev-parse HEAD)" \
  --name "$ETIQUETA" \
  --notes "See the README to install. Binaries and the vocabulary pack hang off this release." \
  >/dev/null
ok "release $ETIQUETA created at $(git rev-parse --short HEAD)"

for f in "$TEMPORAL"/*; do
  glab release upload "$ETIQUETA" "$f" --repo "$REPO" >/dev/null
  ok "uploaded: $(basename "$f")"
done

paso "6. What this release is missing"
# `glab release upload` does not set the asset link's `filepath`, and the CI
# comment says that breaks the `/-/releases/<tag>/downloads/<file>` URL the
# installer uses. Measured on v0.2.0, whose assets have no `filepath`, that
# URL answers 200: glab publishes them under `packages/generic/release-assets`
# and GitLab resolves the download path to it. So it works — but it is a
# behaviour nobody promised us, which is exactly why this check is here
# rather than in a comment.
URL_PRUEBA="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$ARCHIVO"
if curl -fsSL -o /dev/null --retry 2 --connect-timeout 20 "$URL_PRUEBA"; then
  ok "the installer's URL resolves: $URL_PRUEBA"
else
  malo "the installer's URL does NOT resolve: $URL_PRUEBA"
  echo "    The assets are uploaded but the download path does not reach them." >&2
  echo "    Whoever runs the installer will get a 404. Add \`filepath\` to the" >&2
  echo "    asset links, or publish through the pipeline." >&2
  exit 1
fi

FALTAN=""
for p in linux-x86_64 macos-arm64 macos-x86_64; do
  [ "$p" = "$PLATAFORMA" ] && continue
  FALTAN="$FALTAN $p"
done
aviso "this release has no binary for:$FALTAN, and none for Windows"
echo "      macOS:   sh scripts/publicar-mac.sh $ETIQUETA"
echo "      Windows: it needs a Windows machine, or the pipeline"
echo "      On a platform with no binary the installer compiles from source,"
echo "      which works but asks for Rust and a C compiler."

paso "Done."
echo "  https://gitlab.com/$REPO/-/releases/$ETIQUETA"
echo
echo "  Check it the way someone installing it would:"
echo "      sh scripts/instalar.sh --tag $ETIQUETA --prefix /tmp/anathem-check"
