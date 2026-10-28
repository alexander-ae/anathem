#!/bin/sh
# Sube los binarios de macOS a una release de GitLab.
#
# anathem vive en GitLab, pero compilar para macOS exige hardware Apple y los
# runners macOS de GitLab son solo Premium y Ultimate. Así que esa parte la
# hace un repositorio privado en GitHub, y este script es el puente: dispara
# el workflow, espera, baja los artefactos y los sube a la release de GitLab.
#
# El puente es un script y no un token guardado en GitHub a propósito: así no
# hay credenciales de GitLab del lado de GitHub, y cada publicación la hace
# una persona a sabiendas.
#
# Necesita `gh` autenticado en la cuenta dueña del repositorio privado, y
# `glab` autenticado en GitLab.
set -eu

ETIQUETA="${1:-}"
REPO_GH="${ANATHEM_REPO_GH:-alexander-ae/anathem}"
REPO_GL="${ANATHEM_REPO:-__alexander__/anathem}"

if [ -z "$ETIQUETA" ]; then
  echo "Uso: sh scripts/publicar-mac.sh <etiqueta>    (por ejemplo v0.1.0)" >&2
  echo >&2
  echo "Variables: ANATHEM_REPO_GH (donde se compila), ANATHEM_REPO (donde se publica)" >&2
  exit 2
fi

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
malo() { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; }
paso() { printf '\n\033[1m%s\033[0m\n' "$1"; }

for cmd in gh glab; do
  command -v "$cmd" >/dev/null 2>&1 || { malo "falta $cmd en el PATH"; exit 1; }
done

paso "1. Comprobar que la release existe en GitLab"
# Subir a una release inexistente crearía una suelta y sin sentido, así que se
# comprueba antes en vez de descubrirlo al final.
if ! glab release view "$ETIQUETA" --repo "$REPO_GL" >/dev/null 2>&1; then
  malo "no existe la release $ETIQUETA en $REPO_GL."
  echo "    Creala primero: el pipeline de la etiqueta la hace, o" >&2
  echo "    glab release create $ETIQUETA --repo $REPO_GL" >&2
  exit 1
fi
ok "release $ETIQUETA encontrada"

paso "2. Compilar en macOS"
gh workflow run release.yml --repo "$REPO_GH" -f etiqueta="$ETIQUETA"
ok "workflow disparado"

# `gh run watch` necesita el id, y el run recién disparado tarda en aparecer.
CORRIDA=""
INTENTO=0
while [ -z "$CORRIDA" ] && [ "$INTENTO" -lt 30 ]; do
  sleep 2
  CORRIDA="$(gh run list --repo "$REPO_GH" --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)"
  INTENTO=$((INTENTO + 1))
done
[ -n "$CORRIDA" ] || { malo "no encontré la corrida que acabo de disparar"; exit 1; }
ok "corrida $CORRIDA"

if ! gh run watch "$CORRIDA" --repo "$REPO_GH" --exit-status --interval 20; then
  malo "la compilación falló; mirá gh run view $CORRIDA --repo $REPO_GH --log-failed"
  exit 1
fi
ok "compilación terminada"

paso "3. Bajar los artefactos"
TEMPORAL="$(mktemp -d)"
trap 'rm -rf "$TEMPORAL"' EXIT INT TERM
gh run download "$CORRIDA" --repo "$REPO_GH" --dir "$TEMPORAL"
# `gh run download` arma un directorio por artefacto; los juntamos.
find "$TEMPORAL" -type f -name 'mem-macos-*' -exec mv {} "$TEMPORAL"/ \; 2>/dev/null || true
CUANTOS="$(find "$TEMPORAL" -maxdepth 1 -type f -name 'mem-macos-*.tar.gz' | wc -l)"
[ "$CUANTOS" -gt 0 ] || { malo "no bajó ningún binario de macOS"; exit 1; }
ok "$CUANTOS binarios de macOS"

paso "4. Verificar antes de subir"
for t in "$TEMPORAL"/mem-macos-*.tar.gz; do
  s="$t.sha256"
  [ -f "$s" ] || { malo "falta el sha256 de $(basename "$t")"; exit 1; }
  ( cd "$TEMPORAL" && shasum -a 256 -c "$(basename "$s")" >/dev/null 2>&1 \
    || sha256sum -c "$(basename "$s")" >/dev/null 2>&1 ) \
    || { malo "el sha256 de $(basename "$t") no coincide"; exit 1; }
  ok "$(basename "$t"): sha256 correcto"
done

paso "5. Subir a la release de GitLab"
for f in "$TEMPORAL"/mem-macos-*; do
  glab release upload "$ETIQUETA" "$f" --repo "$REPO_GL"
  ok "subido: $(basename "$f")"
done

paso "Listo."
echo "  https://gitlab.com/$REPO_GL/-/releases/$ETIQUETA"
