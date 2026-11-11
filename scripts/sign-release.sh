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

# Downloads one asset, and tells apart "it is not there" from "the server
# said no right now".
#
# This mattered the first time it was run: GitLab answered 429 to every
# request, and the script reported that four of the five files were not in
# the release and that the fifth had no sha256. Both statements were false
# and the release was intact — which is the same failure the publishing
# script already carries a comment about, a warning that was wrong in the
# direction that stops you looking.
#
# `--retry` alone does not cover 429 in every curl, so the status code is
# read and the wait is done here. Only a 404 means absent.
#
# Call it through `if`, never as a bare statement: `set -e` is on, and a
# function that returns non-zero as a simple command ends the script right
# there — the `case $?` after it is never reached, and every branch written
# for "this file is not in the release" or "that was a 429" is dead code
# that looks alive.
bajar() {
  destino="$1"
  url="$2"
  intento=1
  while [ "$intento" -le 5 ]; do
    codigo="$(curl -sSL -o "$destino" -w '%{http_code}' "$url" 2>/dev/null || echo 000)"
    case "$codigo" in
      200) return 0 ;;
      404) return 2 ;;
      429|5??|000)
        [ "$intento" -lt 5 ] && sleep $((intento * 4))
        ;;
      *)
        malo "$url answered $codigo"
        return 1
        ;;
    esac
    intento=$((intento + 1))
  done
  malo "$url still answering $codigo after 5 attempts; it is not a missing file"
  echo "    GitLab rate-limits release downloads. Wait a few minutes and run this again." >&2
  return 1
}

paso "1. Download what the release publishes"

PRESENTES=""
for f in $ARCHIVOS; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f"
  if bajar "$TEMPORAL/$f" "$URL"; then codigo=0; else codigo=$?; fi
  case $codigo in
    0) ok "$f"; PRESENTES="$PRESENTES $f" ;;
    2) aviso "$f is not in this release; skipping it" ;;
    *) exit 1 ;;
  esac
done
[ -n "$PRESENTES" ] || { malo "this release publishes none of the expected files"; exit 1; }

paso "2. Check each one against its own sha256 before signing it"

# Signing a corrupt download would publish a valid signature over the wrong
# bytes, which is worse than not signing: it would carry this key's word.
for f in $PRESENTES; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f.sha256"
  if bajar "$TEMPORAL/$f.sha256" "$URL"; then codigo=0; else codigo=$?; fi
  case $codigo in
    0) : ;;
    2) malo "$f has no .sha256 in the release, so this download cannot be checked"; exit 1 ;;
    *) exit 1 ;;
  esac
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

# A signature already registered is left alone. GitLab answers a duplicate
# asset link with a 400 — "Name has already been taken" — and that aborts
# the whole run, so one file already published takes the rest down with it.
# `publish-binaries.sh` has carried this guard for a while; this script did
# not, and it did not need it until the vocabulary pack started being
# carried over from the previous release **with its signature**. Then the
# pack's `.minisig` was already there, and signing v0.6.0 ended in a red
# error after having correctly signed and uploaded all four binaries.
#
# Being last is what made that harmless. Had the pack been first, it would
# have taken the four binaries with it, and the release would have looked
# signed in the log and been unsigned on the server.
YA="$(glab release view "$ETIQUETA" --repo "$REPO" 2>/dev/null | awk '/^[a-zA-Z0-9._-]+\t/ {print $1}')"
for f in $PRESENTES; do
  if printf '%s\n' "$YA" | grep -qx "$f.minisig"; then
    aviso "$f.minisig is already in the release; leaving it as it is"
    continue
  fi
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
# `bajar` here too, and not a bare `curl -f`. This step used one, and on
# v1.0.1 it said all five signatures did not come back from a release that
# had just stored all five: the uploads above spend the download budget this
# check then needs, so the five requests that follow them are the likeliest
# in the whole script to be answered 429 — and `curl -f` cannot tell a 429
# from a missing file. The script ended with "the release is not properly
# signed", which was false, and whose obvious remedy — sign it again — is
# more requests against the limit that caused it.
ROTOS=0
SINCOMPROBAR=0
for f in $PRESENTES; do
  URL="https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$f.minisig"
  if bajar "$TEMPORAL/bajada-$f.minisig" "$URL"; then codigo=0; else codigo=$?; fi
  case $codigo in
    0) : ;;
    2) malo "$f.minisig is not in the release, although it was just uploaded"
       ROTOS=$((ROTOS + 1)); continue ;;
    *) aviso "$f.minisig could not be downloaded to check it"
       SINCOMPROBAR=$((SINCOMPROBAR + 1)); continue ;;
  esac
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

# Not the same thing, and the difference is the whole point of this block.
# A signature that was uploaded and could not be re-read is probably fine;
# saying otherwise invites signing it again, which is the one action that
# makes a rate limit worse. The release is left as it is and the check is
# left pending, out loud.
if [ "$SINCOMPROBAR" -gt 0 ]; then
  aviso "$SINCOMPROBAR signatures were uploaded but could not be read back just now"
  echo "      Nothing says they are wrong: GitLab was rate-limiting these" >&2
  echo "      downloads, which is what uploading them spends. Do not sign" >&2
  echo "      again. Wait for the limit to clear and run this script once" >&2
  echo "      more; it skips what is already uploaded and only checks." >&2
  exit 1
fi

paso "Done"
echo "  Anyone can now check this release with:"
echo "    sh install.sh --verify-signature"
echo
echo "  If this is the first signed release, put the public key in"
echo "  scripts/install.sh (CLAVE_PUBLICA), in the README, and in the"
echo "  project's description on GitLab."
