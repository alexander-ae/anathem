# anathem

Memoria local para agentes de código (Claude Code, opencode): un binario, un
archivo SQLite, sin daemon ni servidor HTTP. La búsqueda es BM25 por
defecto; una capa vectorial opcional (ver [`mem
model`](#subcomandos) y
[`docs/paquetes-de-vocabulario.md`](docs/paquetes-de-vocabulario.md)) la
complementa cuando instalás un paquete de vocabulario, pero el binario
funciona igual sin ella. Reemplaza a supermemory self-hosted.

## Por qué existe

supermemory corría 2 GB de RSS en reposo (PGlite + ONNX + Rivet) para 390
documentos, de los cuales el 56% era ruido — turnos y conversaciones crudos
compitiendo por los mismos 5 slots de inyección junto con la memoria curada.
El embedder que tenía configurado es solo-inglés y **atomiza y normaliza el
texto a inglés antes de guardarlo**, así que una consulta en español no
comparte ancla léxica ni semántica con el hecho almacenado. Y las
correcciones se acumulaban: la versión vieja seguía activa junto a la nueva.

Medido sobre las mismas 50 consultas en español y el mismo corpus (ground
truth restringido a 171 documentos curados — ver salvedades más abajo):

```
                   recall@5   recall@50   MRR     p50
anathem (BM25)        66%        88%      0.537   5.4 ms
supermemory            24%        34%      0.120   47.2 ms
```

anathem gana en las cuatro métricas. Pero el promedio esconde el hallazgo
real — estratificado por tipo de consulta:

```
                léxicas (n=34)                  parafrásticas (n=16)
anathem         recall@5  88%  recall@50 100%   recall@5  19%  recall@50 62%
supermemory     recall@5  35%  recall@50  50%   recall@5   0%  recall@50  0%
```

BM25 solo resuelve por completo las consultas léxicas (identificadores,
jerga exacta). En las parafrásticas encuentra el documento correcto el 62%
de las veces (recall@50) pero solo lo pone en el top 5 el 19% de las veces
(recall@5) — un problema de **ranking** para los que sí encuentra (la
palanca es un reranker) y un problema de **recuperación** para los que nunca
aparecen en el top 50 (la palanca es una capa vectorial o expansión de
consulta). supermemory, en cambio, no acierta ni una sola de las 16
parafrásticas en el top 50.

**¿Capa vectorial? Ya está, y es opcional.** Esos números eran el insumo de
la pregunta; estos son la respuesta, medidos sobre el mismo corpus con un
paquete de vocabulario instalado:

```
                léxicas (n=34)                  parafrásticas (n=16)
solo BM25       recall@5  88%  recall@50 100%   recall@5  19%  recall@50 62%
con capa        recall@5  88%  recall@50 100%   recall@5  12%  recall@50 75%
```

Hizo lo que se esperaba y solo eso. Las léxicas no se movieron —no había
nada que ganar: ya estaban en 100% de recall@50— y en las parafrásticas
**cerró parte del problema de recuperación**: el recall@50 sube de 62% a
75%, o sea que aparecen documentos que antes no aparecían nunca.

Lo que **no** hizo fue mejorar el ranking. La diferencia de 19% a 12% en
recall@5 es un solo caso sobre dieciséis: ruido, no una regresión. Pero que
no suba confirma el diagnóstico original — encontrar y ordenar son problemas
distintos, y este cambio atacó uno solo. **El reranker sigue pendiente, y
ahora es la restricción que manda.**

Donde la capa sí cambia las cosas de forma grande es entre idiomas, que es
para lo que se construyó: consultando en inglés un corpus escrito en
español, el recall@5 pasa de 34% a 58% (n=50). Ver
[`docs/superpowers/medicion-corpus-real.md`](docs/superpowers/medicion-corpus-real.md).

### Salvedades de esta medición

- **El corpus de benchmark es privado y no se distribuye con este
  repositorio.** Son las notas de trabajo reales del autor original;
  compartirlas violaría la privacidad de terceros mencionados en ellas. Los
  números de arriba son reproducibles en **método** (correr `mem eval`
  contra tu propio corpus y tu propio eval set, con el mismo harness),
  pero no en **datos**: no hay un corpus ni un eval set de referencia
  incluidos para reproducir exactamente estos números.
- El ground truth del eval set lo generó y revisó un agente, no lo confirmó
  todavía un humano.
- "390 documentos" es lo que exponía el listado de documentos de supermemory
  en esa medición; su índice de búsqueda real era mayor — el muestreo
  encontró al menos 488, y cada documento ausente del export que se
  muestreó resultó ser una transcripción cruda (nunca un curado).
  supermemory no pierde por comparar contra un espacio de documentos más
  chico del que en verdad buscaba.

## Instalación

Sin compilar nada, en Linux y macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/alexander-ae/anathem/main/scripts/instalar.sh -o instalar.sh
sh instalar.sh --configurar-path
```

Baja el binario de la última release, verifica su sha256, lo instala, configura
el `PATH`, crea la base y baja el paquete de vocabulario en-es (22 MB). Es
idempotente: correrlo de nuevo no rompe nada.

El `curl` escribe el script en un archivo en vez de mandarlo a `sh` por una
tubería, a propósito: así podés leerlo antes de correrlo, que son 240 líneas.

Para compilar desde fuente —otra plataforma, o preferencia— cloná y agregá
`--compilar`:

```bash
git clone https://github.com/alexander-ae/anathem.git && cd anathem
sh scripts/instalar.sh --configurar-path --compilar
```

Eso sí necesita Rust y un compilador de C; el script se detiene diciendo qué
comando correr si falta alguno. En macOS pide las herramientas de Xcode. Para
Windows, ver [`docs/windows.md`](docs/windows.md).

Paso a paso, y qué hace cada cosa: [`docs/instalacion.md`](docs/instalacion.md).

### Los descargables

Todo cuelga de [las releases](https://github.com/alexander-ae/anathem/releases),
y cada archivo trae su `.sha256` al lado:

| archivo | para |
|---|---|
| `mem-linux-x86_64.tar.gz` | Linux x86_64 |
| `mem-macos-arm64.tar.gz` | macOS con Apple Silicon |
| `mem-macos-x86_64.tar.gz` | macOS con Intel |
| `mem-windows-x86_64.zip` | Windows x86_64 |
| `en-es.anathem-pack` | paquete de vocabulario inglés/español, 22 MB |

El paquete de vocabulario es la capa vectorial, y es opcional: sin él la
búsqueda es solo léxica. El instalador lo baja solo; a mano se instala con
`mem model install <archivo>`.

## Subcomandos

| Comando | Qué hace | Ejemplo |
|---|---|---|
| `init` | Crea la base de datos si no existe | `mem --db ~/.anathem/mem.db init` |
| `migrate --from <jsonl>` | Importa un export congelado de supermemory | `mem migrate --from mi-export.jsonl` |
| `search <query>` | Busca en la memoria (BM25, filtra por `status=active`; si hay un paquete de vocabulario instalado, fusiona el ranking con similitud vectorial) | `mem search "cache breakdown" --project mi-proyecto` |
| `save <texto>` | Guarda una memoria nueva; sin `--project` lo deriva del repositorio donde estés | `mem save "El endpoint /api/x ahora pagina de a 50"` |
| `save <texto> --topic <clave>` | Con clave de tema, la memoria nueva reemplaza a la activa que tenga la misma clave en el mismo proyecto (la vieja queda `superseded`, no se edita ni se borra). Dice a cuál reemplazó. Sin `--topic`, nada cambia | `mem save "El proveedor de pagos es Adyen" --topic proveedor-de-pagos` |
| `topics` | Lista las claves de tema: proyecto, cuántas memorias tuvo cada una y el id de la vigente | `mem topics` |
| `edit <id>` | Edita el cuerpo de una memoria en `$EDITOR` | `mem edit 42` |
| `export --out <dir>` | Exporta todo a markdown con frontmatter, un archivo por memoria, sin perder nada ([formato](docs/formato-de-export.md)) | `mem export --out export/` |
| `reindex` | Reconstruye chunks y el índice FTS5 desde `memories.body` | `mem reindex` |
| `supersede <nueva> <vieja>` | Marca `vieja` como reemplazada por `nueva`; deja de salir en búsquedas | `mem supersede 42 43` |
| `pin <id>` | Fija una memoria para que sobreviva el recorte de candidatos de la búsqueda | `mem pin 17` |
| `unpin <id>` | Quita el pin | `mem unpin 17` |
| `forget <id>` | Saca una memoria de la búsqueda sin borrar la fila | `mem forget 88` |
| `history <id>` | Muestra la cadena de supersession de una memoria, de la más nueva a la más vieja | `mem history 42` |
| `eval --evalset <json>` | Corre el harness de evaluación | `mem eval --evalset eval/evalset.json --out eval/results/anathem-bm25.json` |
| `serve` | Arranca el servidor MCP sobre stdio (4 tools: `search_memory`, `save_memory`, `get_memory`, `list_tags`) | `mem serve` |
| `model install <archivo>` | Instala un paquete de vocabulario (`.anathem-pack`) y reindexa todos los fragmentos | `mem model install mini-es.anathem-pack` |
| `model status` | Muestra el paquete activo, sus dimensiones y la cobertura de vectores | `mem model status` |
| `model remove` | Quita los vectores **y borra el archivo del paquete**; la búsqueda vuelve a ser solo bm25 | `mem model remove` |
| `doctor` | Diagnostica la instalación sin modificar nada: versión y commit del binario, base (conteos, índice FTS5, huérfanos, cadenas de supersession), capa vectorial (paquete, archivo, cobertura) y `config.json`/`tags.json`. Sale con 0 si no hay problemas, 1 si los hay y 2 si no pudo correr, así un script distingue "encontró problemas" de "se rompió"; que no haya paquete instalado es información, no un problema, y una base que no existe es un aviso (código 0) | `mem doctor` |

Todos los subcomandos aceptan `--db <ruta>` (por defecto `~/.anathem/mem.db`).
`mem --version` imprime la versión y el commit con el que se compiló el binario.
Ese commit es el de `HEAD` al compilar: no refleja cambios sin commitear, y `mem doctor` lo
muestra pero no lo compara con nada. Saber si el binario instalado está al día sigue siendo
cosa de quien lo corre: comparalo con `git rev-parse --short HEAD` en el repositorio.
Ver [`docs/paquetes-de-vocabulario.md`](docs/paquetes-de-vocabulario.md) para
el formato del paquete y cómo generarlo.

## Búsqueda en dos etapas

La tool MCP `search_memory` devuelve **20 candidatos con un extracto de ~120
caracteres** cada uno, no unos pocos con el fragmento entero. El agente que la
consume reconoce cuál sirve y pide su cuerpo completo con `get_memory`.

El reordenamiento lo hace el modelo que ya está en la conversación, así que no
cuesta ni disco ni RAM — solo la diferencia de tokens. Medido sobre un corpus
real de 174 memorias y 50 consultas:

| | tokens | candidatos | la memoria correcta está entre los mostrados | ídem, consultas parafrásticas |
|---|---|---|---|---|
| antes | 1.173 | 7,6 | 72 % | 25 % |
| ahora | 1.301 | 20 | **82 %** | **50 %** |

Once por ciento más de tokens por el doble de chances en las consultas
difíciles: las que describen la situación en vez de nombrarla, que son
justamente donde la búsqueda léxica no llega.

El recorte ocurre **antes** de contar el presupuesto de tokens, no al
imprimir. Si fuera al revés, el presupuesto cortaría la lista en ocho
candidatos y el cambio no serviría de nada.

Los dos valores se ajustan en `config.json`, junto a la base:

```json
{ "candidatos": 20, "extracto": 120 }
```

En un corpus de documentos cortos conviene bajar `candidatos`: medido sobre
mMARCO, con pasajes de ~250 caracteres, el techo satura en 10 y pedir más es
desperdicio. Con memorias largas, que es para lo que anathem existe, el salto
de 10 a 20 es el que duplica el techo.

**La CLI no cambió**: `mem search` sigue mostrando los fragmentos completos.

## Aislamiento por proyecto

Cada memoria pertenece a un proyecto, y `search --project` filtra por él. Si
no se pasa `--project` al guardar, se deriva del repositorio donde estés
parado, en este orden:

1. **El nombre del remoto `origin`.** Es el primero a propósito: un
   repositorio suele vivir en una carpeta que no se llama como él
   —`api-monolito` clonado en `API/Web`— y dentro de un worktree la
   carpeta se llama como la rama. El remoto es el único nombre estable entre
   clones, worktrees y máquinas.
2. El nombre de la carpeta del repositorio, si no hay remoto.
3. `sin-proyecto`, fuera de todo repositorio.

Un `--project` explícito siempre gana. El proyecto usado se informa al
guardar, con la marca `(detectado)` cuando no fue elegido a mano: un valor
implícito que no se ve es el que después nadie entiende por qué quedó mal.

No se lee `git` como programa, se lee el archivo `.git` directamente. Así
funciona aunque git no esté instalado y no cuesta un subproceso por guardado.

**La búsqueda NO filtra por proyecto sola.** Se pasa `--project` cuando se
quiere acotar. Filtrar por defecto escondería memorias sin avisar, que es
peor que traer alguna de más.

## Taxonomía de tags

`save_memory` (vía MCP) y `migrate` asignan tags automáticamente clasificando
el cuerpo de la memoria contra un pequeño conjunto de reglas `{tag, patrón}`.
La taxonomía por defecto es deliberadamente chica y genérica — pensada para
cualquiera que lleve notas de ingeniería, sin vocabulario atado a un stack o
equipo en particular: `decision`, `bug`, `convention`, `performance`,
`testing`, `docs`, `code-review`.

Para reemplazarla, poné un archivo `tags.json` junto a la base de datos (en
el mismo directorio que `mem.db`). Sus reglas **reemplazan por completo** a
las por defecto, no se suman. Formato: una lista de objetos `{tag,
pattern}`, donde `pattern` es un regex (sintaxis del crate `regex` de Rust):

```json
[
  { "tag": "urgente", "pattern": "(?i)\\burgente\\b|\\bASAP\\b" },
  { "tag": "cliente",  "pattern": "(?i)\\bcliente\\b" }
]
```

Un archivo malformado (JSON inválido, sin reglas, o con un `pattern` que no
compila como regex) hace que el binario falle con un mensaje que nombra el
archivo y la regla ofensora — nunca cae en silencio a las reglas por
defecto.

## Cómo correr el eval

```bash
mem --db ~/.anathem/mem.db eval --evalset eval/evalset.json --label "anathem bm25" \
  --out eval/results/anathem-bm25.json
```

`eval/evalset.json` y `eval/results/` no están versionados (ver salvedades
arriba: el eval set está armado sobre el corpus privado del autor). `mem
eval` sirve igual como test de regresión sobre tu propio corpus y tu propio
eval set para cualquier cambio futuro al ranking — el formato de
`evalset.json` está documentado en el spec histórico (ver Documentación).

## Limitaciones conocidas

- `migrate` y `save_memory` no asignan tag a un cuerpo que no matchea
  ninguna regla de la taxonomía activa (por defecto o de `tags.json`). Es
  esperado: la taxonomía por defecto es chica a propósito y no pretende
  cubrir todo el conocimiento legítimo de un proyecto (decisiones de
  producto, procedimientos operativos, etc.) — extenderla es una decisión de
  quien opera cada instalación, no un bug.
- Un documento curado que en el export de origen figura con status `failed`
  no se migra: `migrate` lo reporta pero no lo inserta. Su contenido queda
  intacto en el JSONL de origen; hay que reimportarlo a mano con `mem save`.
- `mem supersede` no vincula automáticamente una memoria con su corrección
  posterior: cuál reemplaza a cuál es juicio de quien migra o edita, no algo
  que el binario pueda inferir con seguridad. Lo que sí hace el binario es
  reemplazar cuando quien guarda lo declara con una clave de tema
  (`mem save --topic`, o `topic` en `save_memory`). La clave es exacta y la
  pone quien guarda; una clave nueva no alcanza a una memoria vieja que se
  guardó sin clave; esa se marca con `mem supersede` o, desde MCP, con
  `supersedes: <id>` en `save_memory`.

## Documentación

- Spec y plan de la fase 1 (documentos históricos, escritos contra el
  corpus privado del autor original — ver la nota al inicio de cada uno):
  [`docs/historia/2026-09-21-memoria-local-design.md`](docs/historia/2026-09-21-memoria-local-design.md),
  [`docs/historia/2026-09-21-anathem-fase1.md`](docs/historia/2026-09-21-anathem-fase1.md)
- Insumos técnicos para una fase 2 (reranker / capa vectorial, hallazgos
  diferidos): [`docs/superpowers/fase-2-insumos.md`](docs/superpowers/fase-2-insumos.md)
- Instalación y conexión con los agentes: [`docs/instalacion.md`](docs/instalacion.md)
- Paquetes de vocabulario (capa vectorial opcional): formato, cómo generarlos
  y cómo instalarlos: [`docs/paquetes-de-vocabulario.md`](docs/paquetes-de-vocabulario.md)

## Instalar como plugin de Claude Code

El repositorio es a la vez su propio *marketplace*, así que no hace falta
editar ningún JSON a mano:

```
/plugin marketplace add https://github.com/alexander-ae/anathem.git
/plugin install anathem@anathem
```

**Antes hay que instalar el binario** y dejarlo en el `PATH` como `mem` — ver [`docs/instalacion.md`](docs/instalacion.md). El plugin lo invoca
por nombre y usa `~/.anathem/mem.db` como base, que se crea sola.

Se resolvió así, y no preguntando la ruta con el mecanismo `userConfig` que
Claude Code documenta, porque en la versión 2.1.126 ese mecanismo **no
funciona**: el validador acepta el manifiesto pero el runtime deja
`${user_config.*}` sin resolver y el servidor nunca arranca. Si necesitás otra
ruta de base, usá una entrada manual en `~/.claude.json` en vez del plugin.

Si ya tenías anathem configurado a mano en `~/.claude.json`, **quitá esa
entrada** al instalar el plugin: los dos declaran un servidor con el mismo
nombre.
