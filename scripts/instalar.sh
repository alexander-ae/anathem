#!/bin/sh
# Instalador de anathem. Deliberadamente en `sh` y no en bash: macOS trae
# bash 3.2 de 2007, y la mitad de lo que uno escribe hoy no corre ahí.
#
# Por defecto baja el binario ya compilado de la última release, porque
# compilar Rust es la barrera más alta para quien solo quiere usar la
# herramienta. Con --compilar lo compila desde fuente, que es lo que hace
# falta en una plataforma sin binario publicado.
#
# No hace nada mágico: comprueba, informa y se detiene con un mensaje
# accionable. Es idempotente — correrlo dos veces no rompe nada.
set -eu

REPO="${ANATHEM_REPO:-alexander-ae/anathem}"
PREFIJO="${PREFIJO:-$HOME/.local/bin}"
BASE="${ANATHEM_DB:-$HOME/.anathem/mem.db}"
PAQUETE=""
SIN_PAQUETE=0
CONFIGURAR_PATH=0
COMPILAR=0
ETIQUETA="latest"

uso() {
  cat <<'FIN'
Uso: scripts/instalar.sh [opciones]

  --compilar            compila desde fuente en vez de bajar el binario
  --etiqueta <vX.Y.Z>   instala esa release en vez de la última
  --paquete <archivo>   usa un paquete de vocabulario ya descargado
  --sin-paquete         no instala paquete (búsqueda solo léxica)
  --configurar-path     agrega el directorio de instalación a tu shell
  --prefijo <dir>       dónde instalar el binario (por defecto ~/.local/bin)
  -h, --help            esto

Sin --paquete ni --sin-paquete, baja el paquete en-es de la release.

Variables: PREFIJO, ANATHEM_DB, ANATHEM_REPO
FIN
}

while [ $# -gt 0 ]; do
  case "$1" in
    --compilar) COMPILAR=1; shift ;;
    --etiqueta) ETIQUETA="${2:?--etiqueta necesita una versión}"; shift 2 ;;
    --paquete) PAQUETE="${2:?--paquete necesita un archivo}"; shift 2 ;;
    --sin-paquete) SIN_PAQUETE=1; shift ;;
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
ARQUITECTURA="$(uname -m)"

# Nombre del archivo en la release para esta plataforma, o vacío si no hay.
case "$SISTEMA/$ARQUITECTURA" in
  Linux/x86_64)          PLATAFORMA="linux-x86_64" ;;
  Darwin/arm64)          PLATAFORMA="macos-arm64" ;;
  Darwin/x86_64)         PLATAFORMA="macos-x86_64" ;;
  *)                     PLATAFORMA="" ;;
esac

# Baja $1 a $2. Acepta curl o wget, que entre los dos cubren todo.
bajar() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    malo "falta curl o wget para descargar."
    return 1
  fi
}

# Comprueba el sha256 de $1 contra el contenido de $2.
#
# El sha viene de la misma release que el binario, así que no protege contra
# una release adulterada —para eso haría falta una firma— pero sí contra una
# descarga truncada o corrupta, que es la falla común.
verificar_sha() {
  esperado="$(cut -d' ' -f1 < "$2")"
  if command -v sha256sum >/dev/null 2>&1; then
    obtenido="$(sha256sum "$1" | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    obtenido="$(shasum -a 256 "$1" | cut -d' ' -f1)"
  else
    aviso "sin sha256sum ni shasum: no puedo verificar la descarga"
    return 0
  fi
  if [ "$esperado" != "$obtenido" ]; then
    malo "el archivo bajado no coincide con su sha256; descarga corrupta."
    return 1
  fi
  ok "sha256 verificado"
}

url_release() {
  if [ "$ETIQUETA" = "latest" ]; then
    echo "https://github.com/$REPO/releases/latest/download/$1"
  else
    echo "https://github.com/$REPO/releases/download/$ETIQUETA/$1"
  fi
}

TEMPORAL=""
limpiar() { [ -n "$TEMPORAL" ] && rm -rf "$TEMPORAL" || true; }
trap limpiar EXIT INT TERM

paso "1. Binario"

# Sin binario para esta plataforma no queda más que compilar, lo diga o no.
if [ -z "$PLATAFORMA" ] && [ "$COMPILAR" -eq 0 ]; then
  aviso "no hay binario publicado para $SISTEMA/$ARQUITECTURA; compilo desde fuente"
  COMPILAR=1
fi

if [ "$COMPILAR" -eq 1 ]; then
  if command -v cargo >/dev/null 2>&1; then
    ok "Rust: $(cargo --version)"
  else
    malo "falta Rust para compilar. Instalalo con:"
    echo "      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    echo "    o corré sin --compilar para bajar el binario ya hecho." >&2
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

  cargo build --release --quiet
  ORIGEN="target/release/mem"
  ok "binario compilado"
else
  TEMPORAL="$(mktemp -d)"
  ARCHIVO="mem-$PLATAFORMA.tar.gz"
  ok "plataforma: $SISTEMA/$ARQUITECTURA → $ARCHIVO"
  if ! bajar "$(url_release "$ARCHIVO")" "$TEMPORAL/$ARCHIVO"; then
    malo "no pude bajar el binario. Probá con --compilar, o revisá"
    echo "    https://github.com/$REPO/releases" >&2
    exit 1
  fi
  ok "bajado: $(url_release "$ARCHIVO")"
  if bajar "$(url_release "$ARCHIVO.sha256")" "$TEMPORAL/$ARCHIVO.sha256" 2>/dev/null; then
    verificar_sha "$TEMPORAL/$ARCHIVO" "$TEMPORAL/$ARCHIVO.sha256" || exit 1
  else
    aviso "la release no trae .sha256; sigo sin verificar"
  fi
  tar -C "$TEMPORAL" -xzf "$TEMPORAL/$ARCHIVO"
  ORIGEN="$TEMPORAL/mem"
  [ -f "$ORIGEN" ] || { malo "el archivo bajado no contiene el binario mem"; exit 1; }
fi

paso "2. Instalar"
# mkdir+cp+chmod y no `install -D`: esa bandera es de GNU y no existe en macOS.
mkdir -p "$PREFIJO"
# Renombrar y no copiar encima: si hay un `mem serve` corriendo, copiar encima
# del archivo abierto falla con "Text file busy"; renombrar lo reemplaza y deja
# a los procesos vivos con el inodo viejo hasta que reinicien.
cp "$ORIGEN" "$PREFIJO/mem.nuevo"
chmod 755 "$PREFIJO/mem.nuevo"
mv "$PREFIJO/mem.nuevo" "$PREFIJO/mem"
ok "instalado en $PREFIJO/mem ($("$PREFIJO/mem" --version))"

paso "3. PATH"
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

paso "4. Base de datos"
if [ -f "$BASE" ]; then
  ok "ya existe: $BASE"
else
  "$PREFIJO/mem" --db "$BASE" init >/dev/null
  ok "creada: $BASE"
fi

paso "5. Paquete de vocabulario"
if [ "$SIN_PAQUETE" -eq 1 ]; then
  aviso "sin paquete por pedido: la búsqueda va a ser solo léxica, que funciona"
  aviso "bien para consultas con identificadores pero no entre idiomas."
else
  if [ -z "$PAQUETE" ]; then
    [ -n "$TEMPORAL" ] || TEMPORAL="$(mktemp -d)"
    ok "bajando el paquete en-es (22 MB)"
    if bajar "$(url_release "en-es.anathem-pack")" "$TEMPORAL/en-es.anathem-pack"; then
      if bajar "$(url_release "en-es.anathem-pack.sha256")" "$TEMPORAL/en-es.anathem-pack.sha256" 2>/dev/null; then
        verificar_sha "$TEMPORAL/en-es.anathem-pack" "$TEMPORAL/en-es.anathem-pack.sha256" || exit 1
      fi
      PAQUETE="$TEMPORAL/en-es.anathem-pack"
    else
      aviso "no pude bajar el paquete. La herramienta funciona igual, con"
      aviso "búsqueda solo léxica. Para agregarlo después:"
      echo "      mem --db $BASE model install <archivo.anathem-pack>"
    fi
  fi
  if [ -n "$PAQUETE" ]; then
    if [ ! -f "$PAQUETE" ]; then
      malo "no existe el archivo: $PAQUETE"; exit 1
    fi
    # `model install` copia el paquete a ~/.anathem/paquetes, así que no
    # importa que el origen esté en un temporal que se borra al salir.
    "$PREFIJO/mem" --db "$BASE" model install "$PAQUETE"
    "$PREFIJO/mem" --db "$BASE" model status | sed 's/^/  /'
  fi
fi

paso "6. Diagnóstico"
"$PREFIJO/mem" --db "$BASE" doctor | sed 's/^/  /' || true

paso "Listo. Falta conectarlo a tu agente:"
cat <<FIN
  Claude Code — escribí estos dos comandos adentro de Claude Code:
      /plugin marketplace add https://github.com/$REPO.git
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
