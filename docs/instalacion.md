# Instalación y conexión con los agentes

## 0. La vía corta

No hace falta compilar nada ni instalar Rust:

```bash
curl -fsSL https://raw.githubusercontent.com/alexander-ae/anathem/main/scripts/instalar.sh -o instalar.sh
sh instalar.sh --configurar-path
```

El script detecta tu plataforma, baja el binario de la última release,
verifica su sha256, lo instala, configura el `PATH` de tu shell, crea la base
y baja el paquete de vocabulario en-es. Al final corre `mem doctor` para que
veas que quedó sano. Es idempotente: correrlo de nuevo no rompe nada.

Se escribe el script a un archivo en vez de mandarlo a `sh` por una tubería a
propósito. `curl ... | sh` ejecuta a ciegas lo que conteste el servidor; con el
archivo podés leerlo antes, y son 240 líneas de `sh` sin nada raro.

Hay binarios para Linux x86_64, macOS arm64 (los M1 en adelante), macOS x86_64
y Windows x86_64. En cualquier otra plataforma el script avisa y compila desde
fuente, que necesita Rust y un compilador de C.

Opciones útiles:

```bash
# compilar desde fuente aunque haya binario (necesita clonar el repo)
sh scripts/instalar.sh --configurar-path --compilar

# usar un paquete de vocabulario que ya tenés en disco
sh instalar.sh --configurar-path --paquete ~/Descargas/en-es.anathem-pack

# sin capa vectorial: búsqueda solo léxica, nada que bajar
sh instalar.sh --configurar-path --sin-paquete

# clavar una versión en vez de la última
sh instalar.sh --etiqueta v0.1.0
```

Lo único que el script **no** puede hacer por vos es conectar anathem a tu
agente: los comandos del plugin se escriben adentro de Claude Code. Al
terminar te los imprime.

El resto de este documento explica cada paso por separado, por si preferís
hacerlo a mano o algo falla.

## 1. Instalar el binario a mano

### Bajando el ya compilado

Elegí el archivo de tu plataforma en
[las releases](https://github.com/alexander-ae/anathem/releases) —
`mem-linux-x86_64.tar.gz`, `mem-macos-arm64.tar.gz`, `mem-macos-x86_64.tar.gz`
o `mem-windows-x86_64.zip`— y descomprimilo:

```bash
tar -xzf mem-macos-arm64.tar.gz
mkdir -p ~/.local/bin && mv mem ~/.local/bin/mem && chmod 755 ~/.local/bin/mem
mem --version
```

Cada archivo viene con su `.sha256` al lado. Para comprobarlo:

```bash
shasum -a 256 -c mem-macos-arm64.tar.gz.sha256   # macOS
sha256sum -c mem-macos-x86_64.tar.gz.sha256      # Linux
```

En macOS, la primera vez Gatekeeper puede bloquear un binario bajado de
internet, porque no está firmado con una cuenta de desarrollador de Apple. Se
destraba con `xattr -d com.apple.quarantine ~/.local/bin/mem`, o desde
Ajustes → Privacidad y seguridad, que ofrece abrirlo igual.

### Compilando desde fuente

```bash
git clone https://github.com/alexander-ae/anathem.git anathem
cd anathem
cargo build --release
mkdir -p ~/.local/bin && cp target/release/mem ~/.local/bin/mem && chmod 755 ~/.local/bin/mem
```

(Se usa `mkdir`+`cp`+`chmod` y no `install -D` porque esa bandera es de
GNU coreutils y no existe en el `install` de macOS ni de los BSD.)

Asegurate de que `~/.local/bin` esté en tu `PATH`. El binario pesa unos
10 MB y no depende de nada instalado en el sistema: compila SQLite desde
fuente (`rusqlite` con `bundled`) y trae adentro el tokenizador.

**Hace falta un toolchain de C** para esa compilación. Según el sistema:

| sistema | qué hace falta |
|---|---|
| Linux | `gcc` o `clang`, que suele estar; si no, el paquete `build-essential` o equivalente |
| macOS | las herramientas de línea de comandos de Xcode: `xcode-select --install` |
| Windows | las *Build Tools for Visual Studio* con la carga de C++ — ver [`windows.md`](windows.md) |

### macOS, en concreto

Funciona igual que en Linux —mismas semánticas POSIX— y no hace falta ningún
cambio de código. Tres cosas que sí difieren:

1. **`~/.local/bin` no está en el `PATH` por defecto.** Agregalo a tu
   `~/.zshrc` (zsh es la shell por defecto desde Catalina):
   `export PATH="$HOME/.local/bin:$PATH"`. El plugin de Claude Code invoca al
   binario como `mem`, así que sin esto no arranca.
2. **Xcode Command Line Tools**, como dice la tabla. Si `cargo build` falla en
   `libsqlite3-sys` o en `onig_sys`, es eso y no anathem.
3. **Apple Silicon no necesita nada especial**: Rust compila a
   `aarch64-apple-darwin` por defecto y las dependencias en C también.

Un test se saltea en macOS —el que mide memoria residente, porque lee
`/proc`, que no existe ahí— y lo dice al correr. El resto de la batería corre
completa.

**Probado en macOS**, desde que hay CI: cada push compila en un runner
`macos-14` (Apple Silicon) y corre la batería completa, 250 tests, más un
recorrido de `init`, `save`, `search` y `doctor`. Lo que el CI no cubre es la
instalación en sí —Gatekeeper, el `PATH` de tu shell, el plugin de Claude
Code— así que si algo falla ahí, es información valiosa.

**No hay servicio que levantar.** anathem no es un demonio: no escucha en
ningún puerto, no hay nada que arrancar al prender la máquina, y después de
reiniciar no queda nada corriendo. Todo el estado son archivos. El agente
—Claude Code, opencode— lanza `mem serve` como subproceso cuando arranca y
lo termina al cerrarse.

## 2. Crear la base e importar un corpus (opcional)

```bash
mem --db ~/.anathem/mem.db init
```

Si venís de supermemory (o de cualquier fuente que puedas volcar al mismo
formato JSONL, ver `tools/export_supermemory.py` como referencia), migrá el
export congelado:

```bash
mem --db ~/.anathem/mem.db migrate --from /ruta/a/tu-export.jsonl
```

`migrate` es idempotente: correrlo de nuevo sobre la misma base no duplica
memorias (usa `ext_id` como clave). El comando reporta tres conteos:

```
migradas=<n> archivadas=<n> fallidas=<n>
curadas en status failed (no migradas, reimportar a mano):
  <ext_id>  <título>
```

Un documento curado que el export de origen marcaba como `status: failed` no
se migra automáticamente — su contenido queda intacto en el JSONL. Para
reimportarlo a mano:

```bash
mem --db ~/.anathem/mem.db save "<pegar el contenido de esa memoria desde el JSONL>" \
  --project <proyecto>
```

Si no tenés un corpus para migrar, `mem init` alcanza para arrancar con la
base vacía y empezar a guardar memorias nuevas con `mem save` o con
`save_memory` desde el servidor MCP.

## 3. Instalar el paquete de vocabulario (opcional)

Sin esto anathem busca solo con BM25 y se comporta como siempre. El paquete
agrega búsqueda entre idiomas: encontrar una memoria escrita en español
preguntando en inglés, o al revés. Sobre un corpus real eso lleva el
recall@5 cruzado de 34 % a 58 %, sin perder nada en búsquedas del mismo
idioma.

**El paquete todavía no está publicado en ningún lado.** Es un archivo
binario de 22,5 MB que no corresponde versionar en el repositorio, y aún no
se decidió dónde hospedarlo. Hoy hay dos formas de conseguirlo:

- **Que te lo pasen.** Es un archivo suelto: sirve mandarlo por donde sea.
  No depende del sistema operativo ni de la arquitectura — es una tabla de
  números, igual en Linux, macOS o Windows.
- **Generarlo vos**, con `tools/construir_paquete.py`. Exige bajar el modelo
  base de 512 MB y una muestra de texto en los idiomas que quieras cubrir.
  El procedimiento completo está en
  [`paquetes-de-vocabulario.md`](paquetes-de-vocabulario.md).

Con el archivo en mano:

```bash
mem --db ~/.anathem/mem.db model install en-es.anathem-pack
mem --db ~/.anathem/mem.db model status
```

`status` tiene que decir `cobertura: N/N fragmentos`. Si hay huecos, corré
`mem reindex`. Para volver atrás, `mem model remove` borra los vectores y **también el
archivo del paquete**, y la búsqueda vuelve a ser puramente léxica. Ojo con
eso: si el `.anathem-pack` que instalaste era tu única copia, `remove` la
destruye y reinstalar exige volver a bajarla. Guardá el archivo fuera de
`~/.anathem/paquetes/` si querés poder ir y venir.

El paquete pesa 22,5 MB y es el modelo estático recortado a los idiomas que
cubre. Para generar uno con otros idiomas, ver `paquetes-de-vocabulario.md`.

## 4. El proyecto se detecta solo

Cada memoria pertenece a un proyecto. Al guardar sin `--project`, se deriva
del repositorio donde estés parado: el nombre del remoto `origin`, o el de la
carpeta del repo si no hay remoto, o `sin-proyecto` fuera de todo repositorio.

Se usa el remoto y no el directorio a propósito: un repo suele vivir en una
carpeta que no se llama como él, y dentro de un worktree la carpeta se llama
como la rama. El servidor MCP hereda el directorio del agente que lo lanzó,
así que en la práctica cada sesión de Claude Code guarda en el proyecto
correcto sin que nadie configure nada.

    $ mem save "una nota"
    guardada memoria 42 en api-monolito (detectado)

La marca `(detectado)` distingue lo derivado de lo elegido a mano. La
búsqueda **no** filtra por proyecto sola: se pasa `--project` cuando se
quiere acotar.

## 5. Verificar consumo de recursos

El servidor **no carga el paquete al arrancar**, sino la primera vez que
alguien busca o guarda. Una sesión que trabaja en código y nunca consulta la
memoria no paga nada. Medido:

| momento | RSS |
|---|---|
| servidor arrancado, sin paquete instalado | 7 MB |
| servidor arrancado, con paquete instalado | 7 MB |
| tras `initialize` y `tools/list` | 7 MB |
| tras la primera búsqueda | 64 MB |

Esos 57 MB del salto son casi todos el **tokenizador**, no la tabla de
embeddings: el archivo de 22,5 MB se mapea y solo unos 19 MB quedan
residentes —y esos sí se comparten entre procesos—, mientras que parsear las
76.363 piezas del tokenizador arma estructuras en heap, privadas de cada
proceso.

Por eso importa que la carga sea perezosa: con varias sesiones de agente
abiertas, solo pagan las que de verdad usan la memoria.

## 6. Conectar con Claude Code

Agregá una entrada `mcpServers` a tu configuración de Claude Code —
`~/.claude.json` para que esté disponible en todas tus sesiones, o un
`.mcp.json` en la raíz de un proyecto puntual para alcance por-proyecto:

```json
{
  "mcpServers": {
    "anathem": {
      "command": "/ruta/a/tu/home/.local/bin/mem",
      "args": ["--db", "/ruta/a/tu/home/.anathem/mem.db", "serve"]
    }
  }
}
```

Si el archivo ya tiene otras claves de nivel superior (`env`, `permissions`,
`hooks`, etc.), agregá `mcpServers` como una clave más del mismo objeto — no
reemplaces el archivo entero.

Esto le dice a Claude Code que arranque `mem serve` como un servidor MCP
sobre stdio, apuntado a tu base. Después de guardar el archivo, en una
sesión **nueva** de Claude Code deberían aparecer 4 tools: `search_memory`,
`save_memory`, `get_memory`, `list_tags`. Para verificar, pedile a Claude que
busque algo que ya hayas guardado y confirmá que `search_memory` devuelve
resultados reales.

## 7. Conectar con opencode

El archivo de configuración MCP de opencode es
`~/.config/opencode/opencode.jsonc`. Si ya tiene una clave `"mcp"` de nivel
superior con otros servidores, agregale la entrada de anathem junto a las
que ya están, sin tocar el resto del archivo:

```jsonc
{
  "mcp": {
    "anathem": {
      "type": "local",
      "command": ["/ruta/a/tu/home/.local/bin/mem", "--db", "/ruta/a/tu/home/.anathem/mem.db", "serve"],
      "enabled": true
    }
  }
}
```

Es el mismo binario en modo `serve`, pero con la sintaxis de opencode
(`command` es un array con el binario y sus argumentos, en vez de
`command`/`args` separados como en Claude Code). Verificá igual que en el
paso anterior: sesión nueva de opencode, pedirle que busque algo que ya
hayas guardado, confirmar que trae resultados reales y no un error de
conexión.

## 8. Si venías de supermemory (u otro sistema) y decidís apagarlo

Es una decisión tuya, no algo que anathem resuelva por su cuenta — el
sistema anterior puede ser algo de lo que dependés a diario. Tiene sentido
apagarlo recién después de confirmar, con uso real, que tus agentes están
respondiendo bien con anathem (pasos 4 y 5 de arriba, verificados con
sesiones nuevas de cada uno).

**No borres nada de entrada.** Dejá los datos del sistema anterior en su
lugar — export, blobs, backups — durante un tiempo prudente de uso real de
anathem antes de considerar borrar cualquier cosa. Si algo de la migración
resultara incompleto, esos datos siguen siendo la única copia completa.

## 9. Verificación final

Antes de dar por cerrada la instalación:

```bash
cargo test                       # verde
mem --db ~/.anathem/mem.db export --out /tmp/anathem-export
mem --db ~/.anathem/mem.db reindex
```

`export` debe producir markdown legible con frontmatter, un archivo por
memoria activa/superseded/draft (no `forgotten`), organizado por proyecto.
`reindex` debe reconstruir chunks e índice FTS5 desde `memories.body` sin
errores y sin cambiar el conteo de memorias.
