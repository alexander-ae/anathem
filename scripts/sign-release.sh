#!/bin/sh
# Signs the assets of a published release with minisign, and uploads the
# signatures beside them.
#
# Why by hand, and why here. The signature is supposed to say "the person who
# maintains anathem saw these exact bytes". A key living in CI cannot say
# that: it signs whatever the pipeline produced, which is what the `.sha256`
# already attests, so it would be ceremony rather than a guarantee. On top of
# that, the binaries are not built where they are published: GitHub Actions
# compiles all four platforms and `scripts/publish-binaries.sh` uploads them
# here. A key on either side would attest to a pipeline's work, not to a
# person having seen the bytes that went out.
#
# There is already a manual step with a person's credentials on it:
# publish-binaries.sh. This joins it rather than adding a new one.
#
# Needs minisign and glab, and the release already published with all its
# assets. Run it after publish-binaries.sh.
#
#     sh scripts/sign-release.sh v0.3.0
#
# The first time, generate the key pair and publish the public half:
#
#     minisign -G -p anathem.pub -s ~/.minisign/anathem.key
#
# The private half stays on this machine and nowhere else, and minisign will
# ask for a password to protect it: use one. The file alone should not be
# enough to sign a release.
#
# The public half is one line. It goes into CLAVE_PUBLICA in
# scripts/install.sh, into the README, into the project's description on
# GitLab and into anathem.pub at the root of the repository — four places, so
# a tampered repository on its own cannot swap it without the others
# disagreeing.
set -eu

ETIQUETA="${1:-}"
REPO="${ANATHEM_REPO:-__alexander__/anathem}"
CLAVE="${ANATHEM_SIGNING_KEY:-$HOME/.minisign/anathem.key}"
PAQUETE="anathem"

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
aviso(){ printf '  \033[33m!\033[0m %s\n' "$1" >&2; }
malo() { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso() { printf '\n\033[1m%s\033[0m\n' "$1"; }

if [ -z "$ETIQUETA" ]; then
  echo "Usage: sh scripts/sign-release.sh <tag>    (for example v0.3.0)" >&2
  echo >&2
  echo "Variables: ANATHEM_REPO, ANATHEM_SIGNING_KEY" >&2
  exit 2
fi

for cmd in minisign glab curl; do
  command -v "$cmd" >/dev/null 2>&1 || { malo "$cmd is missing from the PATH"; exit 1; }
done
[ -f "$CLAVE" ] || { malo "no signing key at $CLAVE (see the header of this script)"; exit 1; }

# Everything a release publishes that a person downloads and runs. The
# `.sha256` files are not signed: they describe a file that is itself signed,
# so a signature on them would add a step and no guarantee.
ARCHIVOS="mem-linux-x86_64.tar.gz mem-windows-x86_64.zip mem-macos-arm64.tar.gz mem-macos-x86_64.tar.gz en-es.anathem-pack"

TEMPORAL="$(mktemp -d)"
trap 'rm -rf "$TEMPORAL"' EXIT

paso "1. Download what the release publishes"

PRESENTES=""
for f in $ARCHIVOS; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f"
  if curl -fsSL -o "$TEMPORAL/$f" "$URL" 2>/dev/null; then
    ok "$f"
    PRESENTES="$PRESENTES $f"
  else
    aviso "$f is not in this release; skipping it"
  fi
done
[ -n "$PRESENTES" ] || { malo "this release publishes none of the expected files"; exit 1; }

paso "2. Check each one against its own sha256 before signing it"

# Signing a corrupt download would publish a valid signature over the wrong
# bytes, which is worse than not signing: it would carry this key's word.
for f in $PRESENTES; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f.sha256"
  if ! curl -fsSL -o "$TEMPORAL/$f.sha256" "$URL" 2>/dev/null; then
    malo "$f has no .sha256 in the release, so this download cannot be checked"
    exit 1
  fi
  esperado="$(cut -d' ' -f1 < "$TEMPORAL/$f.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then
    obtenido="$(sha256sum "$TEMPORAL/$f" | cut -d' ' -f1)"
  else
    obtenido="$(shasum -a 256 "$TEMPORAL/$f" | cut -d' ' -f1)"
  fi
  [ "$esperado" = "$obtenido" ] || { malo "$f does not match its sha256; not signing it"; exit 1; }
  ok "$f matches its sha256"
done

paso "3. Sign"

# All of them in one call. `minisign -S` takes several `-m` files and asks
# for the key's password once; one call per file asked five times for the
# same password, which is the kind of friction that gets worked around with
# an unprotected key.
#
# No `-t`: minisign's own trusted comment already carries the timestamp and
# the file name, which is more than a hand-written one would say.
RUTAS=""
for f in $PRESENTES; do
  RUTAS="$RUTAS $TEMPORAL/$f"
done
# shellcheck disable=SC2086
minisign -S -s "$CLAVE" -m $RUTAS
for f in $PRESENTES; do
  [ -f "$TEMPORAL/$f.minisig" ] || { malo "$f was not signed"; exit 1; }
  ok "$f.minisig"
done

paso "4. Upload the signatures and register them in the release"

for f in $PRESENTES; do
  glab release upload "$ETIQUETA" "$TEMPORAL/$f.minisig" --repo "$REPO"
  ok "uploaded $f.minisig"
done

paso "5. Check that each signature comes back, and that it verifies"

# The same lesson as `verificar:release` in the CI: registering a link is not
# publishing a file, and the difference only shows when somebody downloads.
#
# The public key comes from the repository when it is there, and only as a
# last resort from the private one: recovering it with `minisign -R` asks for
# the key's password a second time, and verifying against the file everyone
# else verifies against is the better check anyway — if the committed key
# were wrong, this would catch it.
PUB="$(dirname "$0")/../anathem.pub"
if [ ! -f "$PUB" ]; then
  aviso "no anathem.pub in the repository; recovering it from the private key"
  PUB="$TEMPORAL/anathem.pub"
  minisign -R -s "$CLAVE" -p "$PUB" >/dev/null 2>&1 || true
fi
ROTOS=0
for f in $PRESENTES; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f.minisig"
  if ! curl -fsSL -o "$TEMPORAL/bajada-$f.minisig" "$URL" 2>/dev/null; then
    malo "$f.minisig does not come back from $URL"
    ROTOS=$((ROTOS + 1))
    continue
  fi
  if [ -f "$PUB" ] && minisign -V -p "$PUB" -m "$TEMPORAL/$f" -x "$TEMPORAL/bajada-$f.minisig" >/dev/null 2>&1; then
    ok "$f: published and verifies"
  else
    malo "$f.minisig came back but does not verify"
    ROTOS=$((ROTOS + 1))
  fi
done

if [ "$ROTOS" -gt 0 ]; then
  malo "$ROTOS signatures are not usable; the release is not properly signed"
  exit 1
fi

paso "Done"
echo "  Anyone can now check this release with:"
echo "    sh install.sh --verify-signature"
echo
echo "  If this is the first signed release, put the public key in"
echo "  scripts/install.sh (CLAVE_PUBLICA), in the README, and in the"
echo "  project's description on GitLab."
