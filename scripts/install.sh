#!/bin/sh
# anathem's installer. Deliberately `sh` and not bash: macOS ships bash 3.2
# from 2007, and half of what one writes today does not run there.
#
# By default it downloads the prebuilt binary from the latest release, because
# compiling Rust is the highest barrier for someone who only wants to use the
# tool. With --build it compiles from source, which is what is needed on a
# platform with no published binary.
#
# It does nothing magic: it checks, it reports, and it stops with an actionable
# message. It is idempotent — running it twice breaks nothing.
set -eu

REPO="${ANATHEM_REPO:-__alexander__/anathem}"
PREFIJO="${PREFIJO:-$HOME/.local/bin}"
BASE="${ANATHEM_DB:-$HOME/.anathem/mem.db}"
PAQUETE=""
SIN_PAQUETE=0
CONFIGURAR_PATH=0
CONECTAR=0
COMPILAR=0
SIN_VERIFICAR=0
ETIQUETA="latest"

uso() {
  cat <<'FIN'
Usage: scripts/install.sh [options]

  --build               compile from source instead of downloading the binary
  --tag <vX.Y.Z>        install that release instead of the latest one
  --pack <file>         use an already downloaded vocabulary pack
  --no-pack             install no pack (lexical search only)
  --no-verify           carry on even if the sha256 cannot be checked
  --setup-path          add the install directory to your shell
  --connect             register anathem as an MCP server in Claude Code
  --prefix <dir>        where to install the binary (default ~/.local/bin)
  -h, --help            this

Without --pack or --no-pack, it downloads the en-es pack from the release.

Variables: PREFIJO, ANATHEM_DB, ANATHEM_REPO
FIN
}

# The old Spanish spellings stay accepted alongside the English ones, so a
# script that still passes them keeps working.
while [ $# -gt 0 ]; do
  case "$1" in
    --build|--compilar) COMPILAR=1; shift ;;
    --tag|--etiqueta) ETIQUETA="${2:?--tag needs a version}"; shift 2 ;;
    --pack|--paquete) PAQUETE="${2:?--pack needs a file}"; shift 2 ;;
    --no-pack|--sin-paquete) SIN_PAQUETE=1; shift ;;
    --no-verify|--sin-verificar) SIN_VERIFICAR=1; shift ;;
    --setup-path|--configurar-path) CONFIGURAR_PATH=1; shift ;;
    --connect|--conectar) CONECTAR=1; shift ;;
    --prefix|--prefijo) PREFIJO="${2:?--prefix needs a directory}"; shift 2 ;;
    -h|--help) uso; exit 0 ;;
    *) echo "unknown option: $1" >&2; uso >&2; exit 2 ;;
  esac
done

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
aviso() { printf '  \033[33m!\033[0m %s\n' "$1"; }
malo()  { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

SISTEMA="$(uname -s)"
ARQUITECTURA="$(uname -m)"

# Name of the release file for this platform, or empty if there is none.
case "$SISTEMA/$ARQUITECTURA" in
  Linux/x86_64)          PLATAFORMA="linux-x86_64" ;;
  Darwin/arm64)          PLATAFORMA="macos-arm64" ;;
  Darwin/x86_64)         PLATAFORMA="macos-x86_64" ;;
  *)                     PLATAFORMA="" ;;
esac

# Downloads $1 into $2. Accepts curl or wget, which between them cover
# everything.
#
# The time limits are not decoration: `--retry` only reacts to errors, and a
# download that stalls without failing leaves the installer hanging forever
# without saying anything. It is cut off if it does not connect within 20s, or
# if throughput drops below 1 KB/s for a minute. No overall cap is set because
# the 22 MB pack can legitimately take several minutes on a slow connection.
bajar() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 --connect-timeout 20 --speed-limit 1024 --speed-time 60 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --connect-timeout=20 --read-timeout=60 --tries=3 -O "$2" "$1"
  else
    malo "curl or wget is missing, needed to download."
    return 1
  fi
}

# Checks the sha256 of $1 against the contents of $2.
#
# The sha comes from the same release as the binary, so it does not protect
# against a tampered release —that would take a signature— but it does protect
# against a truncated or corrupt download, which is the common failure.
#
# What it does NOT do is let something through unchecked. Before, when the
# machine had neither sha256sum nor shasum, this function warned and returned
# 0: the installation carried on as if it had verified. An open failure in the
# only integrity control that exists has to stop the installation, not note
# itself down in passing. Whoever genuinely cannot verify says so with
# --no-verify, and then it is a decision and not an accident.
verificar_sha() {
  esperado="$(cut -d' ' -f1 < "$2")"
  if command -v sha256sum >/dev/null 2>&1; then
    obtenido="$(sha256sum "$1" | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    obtenido="$(shasum -a 256 "$1" | cut -d' ' -f1)"
  else
    if [ "$SIN_VERIFICAR" -eq 1 ]; then
      aviso "no sha256sum and no shasum, and --no-verify: carrying on without checking the download"
      return 0
    fi
    malo "cannot find sha256sum or shasum, so the download cannot be checked."
    echo "    Install coreutils (Linux) or use --no-verify if you accept the risk." >&2
    return 1
  fi
  if [ "$esperado" != "$obtenido" ]; then
    malo "the downloaded file does not match its sha256; corrupt download."
    return 1
  fi
  ok "sha256 verified"
}

# The .sha256 of a file we have just downloaded is missing.
#
# Same criterion: a release without its sha is a release that cannot be
# checked, and carrying on in silence turns the control into scenery.
sin_sha_disponible() {
  if [ "$SIN_VERIFICAR" -eq 1 ]; then
    aviso "the release does not ship the .sha256 of $1, and --no-verify: carrying on"
    return 0
  fi
  malo "the release does not ship the .sha256 of $1, so it cannot be checked."
  echo "    Check https://gitlab.com/$REPO/-/releases, or use --no-verify." >&2
  return 1
}

# GitLab resolves `permalink/latest` to the newest release and redirects to the
# file in the package registry. It works without a token on public projects.
url_release() {
  if [ "$ETIQUETA" = "latest" ]; then
    echo "https://gitlab.com/$REPO/-/releases/permalink/latest/downloads/$1"
  else
    echo "https://gitlab.com/$REPO/-/releases/$ETIQUETA/downloads/$1"
  fi
}

TEMPORAL=""
limpiar() { [ -n "$TEMPORAL" ] && rm -rf "$TEMPORAL" || true; }
trap limpiar EXIT INT TERM

paso "1. Binary"

# With no binary for this platform there is nothing left but to compile,
# whether it was asked for or not.
if [ -z "$PLATAFORMA" ] && [ "$COMPILAR" -eq 0 ]; then
  aviso "no published binary for $SISTEMA/$ARQUITECTURA; compiling from source"
  COMPILAR=1
fi

if [ "$COMPILAR" -eq 1 ]; then
  if command -v cargo >/dev/null 2>&1; then
    ok "Rust: $(cargo --version)"
  else
    malo "Rust is missing, needed to compile. Install it with:"
    echo "      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    echo "    or run without --build to download the prebuilt binary." >&2
    exit 1
  fi

  # The project compiles SQLite and oniguruma from source, so it needs cc.
  if [ "$SISTEMA" = "Darwin" ]; then
    if xcode-select -p >/dev/null 2>&1; then
      ok "Xcode command line tools"
    else
      malo "the Xcode tools are missing. Install them with:"
      echo "      xcode-select --install" >&2
      echo "    and run this script again." >&2
      exit 1
    fi
  elif command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
    ok "C compiler"
  else
    malo "a C compiler is missing (gcc or clang). Install build-essential or equivalent."
    exit 1
  fi

  cargo build --release --quiet
  ORIGEN="target/release/mem"
  ok "binary compiled"
else
  TEMPORAL="$(mktemp -d)"
  ARCHIVO="mem-$PLATAFORMA.tar.gz"
  ok "platform: $SISTEMA/$ARQUITECTURA → $ARCHIVO"
  if ! bajar "$(url_release "$ARCHIVO")" "$TEMPORAL/$ARCHIVO"; then
    malo "could not download the binary. Try --build, or check"
    echo "    https://gitlab.com/$REPO/-/releases" >&2
    exit 1
  fi
  ok "downloaded: $(url_release "$ARCHIVO")"
  if bajar "$(url_release "$ARCHIVO.sha256")" "$TEMPORAL/$ARCHIVO.sha256" 2>/dev/null; then
    verificar_sha "$TEMPORAL/$ARCHIVO" "$TEMPORAL/$ARCHIVO.sha256" || exit 1
  else
    sin_sha_disponible "$ARCHIVO" || exit 1
  fi
  # Only the member we care about, and without dragging along the owner
  # recorded in the tar: another machine built that archive and has no business
  # deciding anything about this one.
  tar -C "$TEMPORAL" --no-same-owner -xzf "$TEMPORAL/$ARCHIVO" mem
  ORIGEN="$TEMPORAL/mem"
  [ -f "$ORIGEN" ] || { malo "the downloaded archive does not contain the mem binary"; exit 1; }
fi

paso "2. Install"
# mkdir+cp+chmod and not `install -D`: that flag is GNU's and does not exist on
# macOS.
mkdir -p "$PREFIJO"
# Rename rather than copy over: if a `mem serve` is running, copying over the
# open file fails with "Text file busy"; renaming replaces it and leaves live
# processes on the old inode until they restart.
cp "$ORIGEN" "$PREFIJO/mem.nuevo"
chmod 755 "$PREFIJO/mem.nuevo"
mv "$PREFIJO/mem.nuevo" "$PREFIJO/mem"
ok "installed at $PREFIJO/mem ($("$PREFIJO/mem" --version))"

paso "3. PATH"
if command -v mem >/dev/null 2>&1 && [ "$(command -v mem)" = "$PREFIJO/mem" ]; then
  ok 'the binary is already on the PATH as: mem'
elif [ "$CONFIGURAR_PATH" -eq 1 ]; then
  case "${SHELL:-}" in
    *zsh)  RC="$HOME/.zshrc" ;;
    *bash) RC="$HOME/.bashrc" ;;
    *)     RC="$HOME/.profile" ;;
  esac
  LINEA="export PATH=\"$PREFIJO:\$PATH\""
  if [ -f "$RC" ] && grep -qF "$LINEA" "$RC"; then
    ok "$RC already had the line"
  else
    printf '\n# anathem\n%s\n' "$LINEA" >> "$RC"
    ok "added to $RC — open a new terminal for it to take effect"
  fi
else
  aviso "$PREFIJO is not on your PATH. The Claude Code plugin invokes the"
  aviso "binary as \`mem\`, so without this it will not start. Add to your shell:"
  echo "      export PATH=\"$PREFIJO:\$PATH\""
  echo "    or run again with --setup-path"
fi

paso "4. Database"
if [ -f "$BASE" ]; then
  ok "already exists: $BASE"
else
  "$PREFIJO/mem" --db "$BASE" init >/dev/null
  ok "created: $BASE"
fi

paso "5. Vocabulary pack"
if [ "$SIN_PAQUETE" -eq 1 ]; then
  aviso "no pack, as requested: search will be lexical only, which works well"
  aviso "for queries with identifiers but not across languages."
else
  if [ -z "$PAQUETE" ]; then
    [ -n "$TEMPORAL" ] || TEMPORAL="$(mktemp -d)"
    ok "downloading the en-es pack (22 MB)"
    if bajar "$(url_release "en-es.anathem-pack")" "$TEMPORAL/en-es.anathem-pack"; then
      if bajar "$(url_release "en-es.anathem-pack.sha256")" "$TEMPORAL/en-es.anathem-pack.sha256" 2>/dev/null; then
        verificar_sha "$TEMPORAL/en-es.anathem-pack" "$TEMPORAL/en-es.anathem-pack.sha256" || exit 1
      else
        sin_sha_disponible "en-es.anathem-pack" || exit 1
      fi
      PAQUETE="$TEMPORAL/en-es.anathem-pack"
    else
      aviso "could not download the pack. The tool works all the same, with"
      aviso "lexical search only. To add it later:"
      echo "      mem --db $BASE model install <file.anathem-pack>"
    fi
  fi
  if [ -n "$PAQUETE" ]; then
    if [ ! -f "$PAQUETE" ]; then
      malo "no such file: $PAQUETE"; exit 1
    fi
    # `model install` copies the pack to ~/.anathem/paquetes, so it does not
    # matter that the source sits in a temporary directory erased on exit.
    "$PREFIJO/mem" --db "$BASE" model install "$PAQUETE"
    "$PREFIJO/mem" --db "$BASE" model status | sed 's/^/  /'
  fi
fi

paso "6. Connect to the agents"
if [ "$CONECTAR" -eq 1 ] && command -v claude >/dev/null 2>&1; then
  # `claude mcp add` fails if the name already exists, and that is not an error
  # here: the script has to be runnable twice without breaking anything.
  if claude mcp list 2>/dev/null | grep -q '^anathem'; then
    ok "Claude Code already had anathem registered"
  elif claude mcp add --scope user anathem -- "$PREFIJO/mem" --db "$BASE" serve >/dev/null 2>&1; then
    ok "registered in Claude Code (scope: user)"
    aviso "open a new Claude Code session for the tools to show up"
  else
    aviso "could not register it in Claude Code. By hand:"
    echo "      claude mcp add --scope user anathem -- $PREFIJO/mem --db $BASE serve"
  fi
elif [ "$CONECTAR" -eq 1 ]; then
  aviso "could not find the \`claude\` command on the PATH; skipping the registration"
fi

# opencode accepts both names and the official documentation mentions `.json`,
# but many installations have `.jsonc` because it allows comments. Whichever
# exists is looked up; `OPENCODE_CONFIG` wins over both, as in opencode itself.
OC=""
for candidato in "${OPENCODE_CONFIG:-}" "$HOME/.config/opencode/opencode.jsonc" "$HOME/.config/opencode/opencode.json"; do
  if [ -n "$candidato" ] && [ -f "$candidato" ]; then OC="$candidato"; break; fi
done
ENTRADA_OC="    \"anathem\": { \"type\": \"local\", \"command\": [\"$PREFIJO/mem\", \"--db\", \"$BASE\", \"serve\"], \"enabled\": true },"

if [ -z "$OC" ] && [ "$CONECTAR" -eq 1 ] && command -v opencode >/dev/null 2>&1; then
  # opencode installed and not configured yet: there is no file to edit, so one
  # is created. This is the case of a fresh installation, and skipping it in
  # silence left opencode without anathem with nobody the wiser.
  OC="$HOME/.config/opencode/opencode.json"
  mkdir -p "$(dirname "$OC")"
  cat > "$OC" <<FIN
{
  "\$schema": "https://opencode.ai/config.json",
  "mcp": {
    "anathem": { "type": "local", "command": ["$PREFIJO/mem", "--db", "$BASE", "serve"], "enabled": true }
  }
}
FIN
  ok "created $OC with anathem"
  aviso "open a new opencode session for the tools to show up"
  OC=""
elif [ -z "$OC" ] && [ "$CONECTAR" -eq 1 ]; then
  : # opencode is not installed; there is nothing to say.
fi

if [ -n "$OC" ]; then
  if grep -q '"anathem"' "$OC" 2>/dev/null; then
    ok "opencode already had anathem configured"
  elif [ "$CONECTAR" -eq 0 ]; then
    aviso "opencode is installed but without anathem. Run with --connect, or add to its \"mcp\":"
    echo "  $ENTRADA_OC"
  else
    # A line is inserted after the `mcp` key instead of rewriting the file with
    # a JSON parser: that format allows comments and a parser would eat them.
    # The insertion is purely additive.
    #
    # It is only done if a line ends exactly at `"mcp": {`. The brace is
    # required to close the line because with `"mcp": {}` on a single one —or
    # any compact form— inserting the next line would leave the entry outside
    # the object and break the file. With zero matches there is nowhere to put
    # it, and with more than one there is no way to know which is the right one.
    # `grep -c` prints 0 and also exits with 1 when it finds nothing, so an
    # `|| echo 0` would leave the variable at "0\n0" and the `[` below would
    # fail.
    CUANTAS=$(grep -c '"mcp"[[:space:]]*:[[:space:]]*{[[:space:]]*$' "$OC" 2>/dev/null) || CUANTAS=0
    if [ "$CUANTAS" -eq 1 ]; then
      cp "$OC" "$OC.bak-anathem"
      # The whole file is read so the following line can be looked at: if the
      # `mcp` object is empty, the entry goes in without a trailing comma,
      # because a comma before `}` is not valid JSON even though some readers
      # forgive it.
      awk -v entrada="$ENTRADA_OC" '
        { linea[NR] = $0 }
        END {
          for (i = 1; i <= NR; i++) {
            print linea[i]
            if (hecho || linea[i] !~ /"mcp"[[:space:]]*:[[:space:]]*\{[[:space:]]*$/) continue
            for (j = i + 1; j <= NR; j++) {
              if (linea[j] ~ /^[[:space:]]*$/ || linea[j] ~ /^[[:space:]]*\/\//) continue
              vacio = (linea[j] ~ /^[[:space:]]*\}/)
              break
            }
            if (vacio) { sub(/,$/, "", entrada) }
            print entrada
            hecho = 1
          }
        }
      ' "$OC.bak-anathem" > "$OC.nuevo-anathem" && mv "$OC.nuevo-anathem" "$OC"
      ok "added to opencode (previous copy at $OC.bak-anathem)"
      aviso "open a new opencode session for the tools to show up"
    else
      aviso "could not find an \"mcp\" key opening at the end of a line in"
      aviso "$OC; leaving it untouched so as not to break it."
      aviso "Add this entry inside its \"mcp\":"
      echo "  $ENTRADA_OC"
    fi
  fi
fi

paso "7. Diagnosis"
"$PREFIJO/mem" --db "$BASE" doctor | sed 's/^/  /' || true

paso "Done. It still needs connecting to your agent:"
cat <<FIN
  Claude Code — type these two commands inside Claude Code:
      /plugin marketplace add https://gitlab.com/$REPO.git
      /plugin install anathem@anathem

  opencode — if it was not configured automatically, add to your
  ~/.config/opencode/opencode.json (or .jsonc), inside "mcp":
      "anathem": {
        "type": "local",
        "command": ["$PREFIJO/mem", "--db", "$BASE", "serve"],
        "enabled": true
      }

  Then check that it works:
      mem --db $BASE save "a test note"
      mem --db $BASE search "test"
FIN
