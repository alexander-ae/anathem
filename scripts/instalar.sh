#!/bin/sh
# Instalador de anathem. Deliberadamente en `sh` y no en bash: macOS trae
# bash 3.2 de 2007, y la mitad de lo que uno escribe hoy no corre ahí.
#
# No hace nada mágico: comprueba, informa y se detiene con un mensaje
# accionable. Es idempotente — correrlo dos veces no rompe nada.
set -eu

PREFIJO="${PREFIJO:-$HOME/.local/bin}"
BASE="${ANATHEM_DB:-$HOME/.anathem/mem.db}"
PAQUETE=""
CONFIGURAR_PATH=0

uso() {
  cat <<'FIN'
Uso: scripts/instalar.sh [opciones]

  --paquete <archivo>   instala también el paquete de vocabulario
  --configurar-path     agrega el directorio de instalación a tu shell
  --prefijo <dir>       dónde instalar el binario (por defecto ~/.local/bin)
  -h, --help            esto

Variables: PREFIJO, ANATHEM_DB
FIN
}

while [ $# -gt 0 ]; do
  case "$1" in
    --paquete) PAQUETE="${2:?--paquete necesita un archivo}"; shift 2 ;;
    --configurar-path) CONFIGURAR_PATH=1; shift ;;
    --prefijo) PREFIJO="${2:?--prefijo necesita un directorio}"; shift 2 ;;
    -h|--help) uso; exit 0 ;;
    *) echo "opción desconocida: $1" >&2; uso >&2; exit 2 ;;
  esac
done

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
aviso() { printf '  \033[33m!\033[0m %s\n' "$1"; }
malo()  { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

SISTEMA="$(uname -s)"

paso "1. Requisitos"

if command -v cargo >/dev/null 2>&1; then
  ok "Rust: $(cargo --version)"
else
  malo "falta Rust. Instalalo con:"
  echo "      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  echo "    y volvé a correr este script." >&2
  exit 1
fi

# El proyecto compila SQLite y oniguruma desde fuente, así que necesita cc.
if [ "$SISTEMA" = "Darwin" ]; then
  if xcode-select -p >/dev/null 2>&1; then
    ok "herramientas de línea de comandos de Xcode"
  else
    malo "faltan las herramientas de Xcode. Instalalas con:"
    echo "      xcode-select --install" >&2
    echo "    y volvé a correr este script." >&2
    exit 1
  fi
elif command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
  ok "compilador de C"
else
  malo "falta un compilador de C (gcc o clang). Instalá build-essential o equivalente."
  exit 1
fi

paso "2. Compilar"
cargo build --release --quiet
ok "binario compilado"

paso "3. Instalar"
# mkdir+cp+chmod y no `install -D`: esa bandera es de GNU y no existe en macOS.
mkdir -p "$PREFIJO"
cp target/release/mem "$PREFIJO/mem"
chmod 755 "$PREFIJO/mem"
ok "instalado en $PREFIJO/mem"

paso "4. PATH"
if command -v mem >/dev/null 2>&1 && [ "$(command -v mem)" = "$PREFIJO/mem" ]; then
  ok 'el binario ya se encuentra en el PATH como: mem'
elif [ "$CONFIGURAR_PATH" -eq 1 ]; then
  case "${SHELL:-}" in
    *zsh)  RC="$HOME/.zshrc" ;;
    *bash) RC="$HOME/.bashrc" ;;
    *)     RC="$HOME/.profile" ;;
  esac
  LINEA="export PATH=\"$PREFIJO:\$PATH\""
  if [ -f "$RC" ] && grep -qF "$LINEA" "$RC"; then
    ok "$RC ya tenía la línea"
  else
    printf '\n# anathem\n%s\n' "$LINEA" >> "$RC"
    ok "agregado a $RC — abrí una terminal nueva para que tome efecto"
  fi
else
  aviso "$PREFIJO no está en tu PATH. El plugin de Claude Code invoca el"
  aviso "binario como \`mem\`, así que sin esto no arranca. Agregá a tu shell:"
  echo "      export PATH=\"$PREFIJO:\$PATH\""
  echo "    o volvé a correr con --configurar-path"
fi

paso "5. Base de datos"
if [ -f "$BASE" ]; then
  ok "ya existe: $BASE"
else
  "$PREFIJO/mem" --db "$BASE" init >/dev/null
  ok "creada: $BASE"
fi

paso "6. Paquete de vocabulario"
if [ -n "$PAQUETE" ]; then
  if [ ! -f "$PAQUETE" ]; then
    malo "no existe el archivo: $PAQUETE"; exit 1
  fi
  "$PREFIJO/mem" --db "$BASE" model install "$PAQUETE"
  "$PREFIJO/mem" --db "$BASE" model status | sed 's/^/  /'
else
  aviso "sin paquete: la búsqueda va a ser solo léxica, que funciona bien"
  aviso "para consultas con identificadores. Para buscar entre idiomas,"
  aviso "conseguí un .anathem-pack y corré:"
  echo "      mem --db $BASE model install <archivo.anathem-pack>"
fi

paso "Listo. Falta conectarlo a tu agente:"
cat <<FIN
  Claude Code — escribí estos dos comandos adentro de Claude Code:
      /plugin marketplace add https://github.com/alexander-ae/anathem.git
      /plugin install anathem@anathem

  opencode — agregá a ~/.config/opencode/opencode.jsonc, dentro de "mcp":
      "anathem": {
        "type": "local",
        "command": ["$PREFIJO/mem", "--db", "$BASE", "serve"],
        "enabled": true
      }

  Después, probá que anda:
      mem --db $BASE save "una nota de prueba"
      mem --db $BASE search "prueba"
FIN
