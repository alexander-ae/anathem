> **Documento histórico.** Este spec se escribió durante el diseño original de
> anathem, contra el corpus privado de notas de trabajo de su autor original
> (nombres de proyecto, tickets, colegas y fragmentos de ese corpus incluidos).
> Se conserva sin reescribir como registro del razonamiento y las métricas que
> motivaron las decisiones de diseño — no describe el comportamiento genérico
> del proyecto tal como se publica. Para eso, ver el [README](../../README.md).

# anathem — sistema de memoria local para agentes

Diseño. Fecha: 2026-09-21. Estado: propuesto, pendiente de revisión.
Repo: `git@github.com:alexander-ae/anathem.git`

## 1. Problema

Hoy la memoria de los agentes (Claude Code y opencode) vive en supermemory
self-hosted. Medido el 2026-09-21 sobre la instancia local:

| Métrica | Valor |
|---|---|
| Memorias almacenadas | 390 |
| RSS del proceso | 2.048 MB |
| Disco | 665 MB (175 MB datos + 267 MB runtime + 106 MB modelos) |
| Runtime | PGlite (Postgres en WASM) + ONNX + Rivet |
| Formato en disco | blob único cifrado, cabecera `SMD1` |

Cuatro defectos concretos, todos verificados:

1. **Sin aislamiento por proyecto.** Las 390 memorias están en un solo
   container, `my-project-tag` — el valor de ejemplo del comentario de
   `~/.config/opencode/supermemory.jsonc`, que quedó puesto y sobreescribe el
   tag auto-generado.
2. **El 56% es ruido.** 153 memorias curadas contra 218 volcados crudos de
   turnos y conversaciones, más 16 de configuración. Todas compiten por los
   mismos 5 slots de inyección.
3. **Las correcciones se acumulan en vez de reemplazar.** Existen varias
   memorias que empiezan con `CORRECCION al save anterior`; la versión
   incorrecta sigue activa y ambas se recuperan.
4. **La capa vectorial resta.** El modelo configurado es
   `Xenova/bge-base-en-v1.5`, que es solo inglés, sobre un corpus en español.

### Evidencia sobre el punto 4

Mini-eval de 12 consultas en español sobre el corpus real (171 documentos
curados, 1.045 chunks), ground truth a nivel documento:

| Configuración | recall@5 | MRR |
|---|---|---|
| BM25 (FTS5) | 10/12 (83%) | 0.565 |
| **bge-base-en-v1.5 [768d] — actual** | **7/12 (58%)** | **0.413** |
| paraphrase-multilingual-MiniLM-L12 [384d] | 9/12 (75%) | 0.500 |
| Híbrido RRF (BM25 + multilingüe) | 11/12 (92%) | 0.701 |

Una de las 12 consultas tiene ground truth vacío, así que el techo alcanzable
es 11: el híbrido acertó todo lo acertable.

Dos salvedades. El ground truth se construyó por regex, lo que favorece
estructuralmente a BM25 — la ventaja real del multilingüe sobre el inglés es
probablemente mayor que la medida. Y n=12 es direccional, no concluyente.

### Baseline medido contra el sistema real

Head to head sobre el **mismo corpus de 390 documentos**, las mismas 12
consultas y el mismo ground truth:

| Sistema | recall@5 | recall@50 | MRR | p50 |
|---|---|---|---|---|
| supermemory `/v3/search` | 5/12 (42%) | 6/12 (50%) | 0.205 | 54 ms |
| BM25 solo (FTS5) | 10/12 (83%) | 12/12 (100%) | 0.780 | 15 ms |
| Vectorial multilingüe | 10/12 (83%) | 12/12 (100%) | 0.604 | 9 ms |
| Híbrido RRF | 10/12 (83%) | 12/12 (100%) | 0.773 | 39 ms |

Lo decisivo es recall@50. Aplicando la tabla de diagnóstico de la sección 9,
supermemory tiene recall@50 bajo **y** recall@5 bajo: no encuentra los
documentos, no es que los ordene mal. Un reranker no lo corregiría. Las tres
alternativas encuentran el 100% dentro de los primeros 50.

En las 12 consultas los top-5 de supermemory se llenaron casi siempre de
documentos crudos (0/5 o 1/5 curados). El 56% de ruido le está consumiendo
los resultados.

**Resultado que contradice la hipótesis inicial:** sobre los 390, el híbrido
no le gana a BM25 solo (MRR 0.773 contra 0.780, mismo recall@5). La capa
vectorial no se pagó a sí misma en esta medición. Salvedades en ambas
direcciones: n=12 es poco, y el ground truth por regex favorece
estructuralmente a BM25.

### Corrección sobre el mecanismo de falla

La hipótesis inicial era que el embedder inglés no casaba con un corpus en
español. El mecanismo es otro. La API `/v4/search` muestra que supermemory
**atomiza y normaliza a inglés** antes de embeber:

```
"The API endpoint Api::Brokers::BillsController#breakdown causes
 215 MB of memory bloat per request."
```

Su embedder inglés es coherente con su propio almacén. El desajuste real es
que **la consulta va en español y el hecho almacenado está en inglés**.
Mismo síntoma, causa distinta.

## 2. Objetivos

- Un binario, un archivo de base de datos, sin daemon ni servidor.
- RAM en reposo de dos órdenes de magnitud por debajo de los 2 GB actuales.
- Recuperación híbrida validada por medición, no por criterio.
- Memorias corregibles a mano y auditables en el tiempo.
- Una sola base para Claude Code y opencode, reemplazando los dos silos
  actuales (memoria nativa de Claude + supermemory).

### Fuera de alcance

Ingesta multi-fuente (web, PDFs, Notion), UI, sincronización entre
dispositivos, multi-usuario, despliegue remoto.

## 3. Decisiones

| # | Decisión | Razón |
|---|---|---|
| D1 | Migrar solo las 171 curadas; las 218 crudas a `archive.jsonl` sin indexar | El 56% de ruido es la causa de que los 5 slots rindan poco |
| D2 | Proyecto como filtro blando + tags multi-etiqueta | El 85% del corpus es un proyecto: proyecto no discrimina, tema sí; y una memoria toca varios temas |
| D3 | Guardado explícito **inmediato**, más sugerencia en lote al cierre | Produce el tipo de memoria que hoy funciona; el clasificador no queda residente |
| D4 | SQLite como fuente de verdad, con sqlite-vec y FTS5 | Medido: 0,36 ms por consulta a esta escala; fuerza bruta sobra |
| D5 | Eval de ~50 casos **antes** de construir, con baseline contra supermemory | Sin baseline no hay forma de saber si el reemplazo mejora |
| D6 | Migrar las 171 tal cual, sin atomizar, y medir el daño | La atomización es costosa e irreversible; se justifica con números o no se hace |
| D7 | La capa vectorial es **condicional**, no asumida | Medido: sobre los 390 el híbrido empata con BM25 solo. Entra solo si supera a BM25 en el eval de 50 casos |

### Medición que respalda D4

`sqlite-vec` v0.1.9, KNN k=5, 384 dimensiones, fuerza bruta:

| N chunks | insert | p50 | p95 |
|---|---|---|---|
| 1.045 (escala actual) | 0,04 s | 0,36 ms | 0,38 ms |
| 10.000 | 0,23 s | 1,95 ms | 2,06 ms |
| 100.000 | 2,33 s | 23,15 ms | 23,57 ms |
| 500.000 | 11,68 s | 114,66 ms | 115,44 ms |

Con filtro por metadata sobre `project`: 0,78 ms a 1.045 chunks, 82 ms a
100.000.

Se requeriría un crecimiento de ~100× para que el índice aproximado importe.
No se usa ANN, ni DiskANN, ni libSQL.

Tamaño en disco con el corpus real: 0,9 MB de texto más FTS5, 0,4 MB de
vectores int8 a 384d. Total ~2,5 MB (más ~220 MB del modelo).

## 4. Arquitectura

```
binario Rust único
  ├── SQLite vía rusqlite (compilado adentro)     fuente de verdad
  │     ├── FTS5 unicode61 remove_diacritics      BM25
  │     ├── sqlite-vec vec0 384d                  KNN + filtro por metadata
  │     └── supersedes / tags / refs
  ├── fastembed-rs, embedder multilingüe 384d     bajo demanda, se libera
  ├── fusión RRF                                  BM25 + vectorial
  ├── servidor MCP stdio                          Claude Code y opencode
  └── CLI                                         edit, export, reindex, eval
```

Presupuesto de RAM: ~20 MB en reposo, ~170 MB durante un guardado o una
consulta. Contra 2.048 MB hoy.

## 5. Esquema

```sql
memories(
  id, project, title, body, created, updated,
  event_start, event_end,   -- cuándo ocurrió el hecho, distinto de cuándo se guardó
  source,                   -- explicit | suggested | migrated
  status,                   -- active | superseded | draft | forgotten
  pinned,                   -- siempre candidata para su proyecto
  last_accessed, access_count,
  supersedes_id
)
memory_tags(memory_id, tag)
memory_refs(memory_id, kind, value)   -- kind: ticket | pr | sha
chunks(id, memory_id, ord, body)
search_log(id, ts, query, project, returned_ids, budget_used, followed_up)
chunks_fts                             -- FTS5 external content sobre chunks.body
vec_chunks                             -- vec0: embedding float[384], +memory_id, +project
```

Se indexa por chunk y se rankea y deduplica **por memoria**, para que una
memoria larga no ocupe los cinco resultados con cinco pedazos de sí misma.

`memory_refs` sale del supuesto abierto 1 (sección 10): los identificadores
`OP-###`, `PR #`, y SHAs se extraen a una tabla en vez de convertirse en tags,
y habilitan búsqueda exacta por ticket.

### Taxonomía inicial de tags

Derivada del conteo de señales sobre el corpus:

| Tag | Señal |
|---|---|
| `perf` | 202 menciones de memoria/MB, 104 de N+1/query |
| `code-review` | 100 menciones de PRs, más los nombres de los revisores recurrentes |
| `testing` | 133 specs/rspec |
| `migraciones` | 59 |
| `tooling-memoria` | 33 supermemory |
| `ux-design` | 23 |
| `infra` | 18 cron |

Se asignan por regex en el bootstrap y se corrigen a mano después.

### Observabilidad

`search_log` registra cada consulta: el texto, los ids devueltos, los tokens
gastados, y si el agente después llamó `get_memory` — señal de que el
fragmento inyectado no alcanzó.

El propósito es convertir el uso diario en datos de evaluación. El eval de 50
casos es el punto de partida; el log es lo que deja seguir midiendo después,
sin tener que inventar consultas.

## 6. Recuperación e inyección

Búsqueda: BM25 y KNN en paralelo, 30 candidatos cada uno, fusión por RRF
(K=60), deduplicación por memoria, filtro por `project` y `tags` si se piden.
Las memorias con `status = 'superseded'` quedan excluidas por defecto.

Inyección, en tokens y no en cantidad de memorias:

```
presupuesto      ~1.200 tokens por turno
por resultado    título + descripción + el chunk que hizo match  (~150-250 tokens)
                 más el id
bajo demanda     get_memory(id) devuelve el cuerpo completo
```

Hoy son 5 memorias enteras a ~550 tokens de mediana, unos 2.750 tokens por
turno. El esquema propuesto entrega más memorias distintas en menos tokens y
deja que el agente decida si profundiza.

## 7. Correcciones y supersession

Al guardar se compara la memoria nueva contra las 5 más cercanas. Si hay
reemplazo:

```
nueva.supersedes_id = vieja.id
vieja.status        = 'superseded'
```

Las superseded no se recuperan pero no se borran. `mem history <id>` muestra
la cadena completa.

En v1 la detección es **asistida**: el sistema propone el vínculo y el usuario
confirma. Automatizarla es precisamente lo que se evalúa para el clasificador
(sección 9).

## 8. Interfaces

```
MCP     search_memory(query, project?, tags?, budget?)
        save_memory(text, project?, tags?)
        get_memory(id)
        list_tags()

CLI     mem search [--explain] | save | edit <id> | history <id>
        mem pin <id> | forget <id>
        mem export [--git]     markdown en el formato de CLAUDE.md
        mem reindex            reconstruye FTS5 y vectores desde memories
        mem eval               corre el harness
```

Cuatro tools MCP deliberadamente: cada una pesa en el contexto de todos los
turnos.

`mem edit` abre `$EDITOR` y `mem export` vuelca markdown versionable en git.
Eso cubre la corregibilidad sin que los archivos sean la fuente de verdad.

`--explain` muestra score BM25, score vectorial y aporte al RRF por resultado;
sin eso, afinar la fusión es a ciegas. `pin` marca una memoria como siempre
candidata para su proyecto y `forget` la retira de la recuperación sin
borrarla: son la válvula de escape cuando el ranking se equivoca y no se
quiere esperar a retunearlo.

## 9. Harness de evaluación

Es el **primer entregable**, antes del binario.

- ~50 consultas en español derivadas de las 171 memorias, con ground truth a
  nivel documento revisado por el usuario.
- Métricas: **recall@5, recall@50 y MRR**, más tokens inyectados. Los tres
  juntos: sin recall@50 no se distingue un problema de ranking de uno de
  recuperación, y sin esa distinción se elige palanca a ciegas.
- Baseline obligatorio contra la instancia de supermemory tal como está hoy.
- Queda como test de regresión de todo cambio posterior al ranking.

### Diagnóstico

| recall@50 | recall@5 | Problema | Palanca |
|---|---|---|---|
| alto | bajo | El documento se encuentra pero queda enterrado | reranker cross-encoder |
| bajo | bajo | El documento no aparece nunca | chunking, embedder, expansión de query |

Falla en consultas parafraseadas → embedder. Falla en consultas con jerga o
identificadores → subir el peso léxico en la fusión.

### Palancas, por costo creciente

1. Chunking: 300 / 500 / 800 / por párrafo. Hoy 500/400, elegido sin criterio.
   Suele ser la palanca más grande.
2. Pesos de la fusión y número de candidatos.
3. Tokenizer: stemmer español por snowball, o trigram para tolerar tipeos.
4. Embedder: `multilingual-e5-small` con prefijos, o `bge-m3`.
   **Cuidado:** la familia e5 exige los prefijos `query: ` y `passage: `; sin
   ellos el recall cae y se diagnostica mal el modelo.
5. Reranker cross-encoder (`bge-reranker-v2-m3`) sobre los top-20. Es la
   palanca grande de MRR, y solo aplica si el diagnóstico dice "ranking".
6. Expansión de query por el agente antes de buscar.
7. Atomizar memorias al guardar. Pospuesto para v1 por D6, **no descartado**:
   supermemory ya lo hace, así que es una técnica con precedente en este mismo
   corpus. El eval de 50 casos debe probarla explícitamente como variante.
   Nota: supermemory además traduce a inglés al atomizar, y eso *no* se copia
   — la medición sugiere que el cruce de idiomas es parte de su problema.

### Sobre Jev / Kev

`Kev-0.5B` es un adaptador LoRA más un pointer head sobre Qwen2.5-0.5B: 9,3M
parámetros entrenables, el 1,9% del backbone, entrenado en 1h45 en un MacBook
Pro M5. Lee un documento una vez y responde varias preguntas tipadas en un
solo prefill, sin decodificar.

No se adopta: está entrenado en inglés y Qwen2.5-0.5B es débil en español.

Sí se adopta la forma. Para asignar tags de un conjunto fijo no hace falta un
modelo de decisión: una **regresión logística multi-etiqueta sobre los mismos
384 floats del embedder**, entrenada con las memorias ya etiquetadas, ocupa
unos KB, corre en microsegundos y no agrega RAM. Es el pointer head de Kev
sobre un backbone más chico y con datos propios.

La detección de supersession es razonamiento sobre dos textos y probablemente
excede a un clasificador lineal. Si el eval lo justifica, ahí sí cabe
llama.cpp con gramática GBNF restringiendo la salida al conjunto de opciones y
leyendo logprobs con `max_tokens=1` — acotado a las 5 memorias más cercanas y
solo en el momento de guardar.

## 10. Supuestos abiertos

Decididos por criterio, pendientes de confirmación:

1. **Los identificadores no son tags.** `OP-###`, `PR #` y SHAs van a
   `memory_refs`. Con 498 menciones repartidas en 171 documentos no
   discriminan como tema.
2. **Presupuesto de inyección: ~1.200 tokens.** Es una estimación, no una
   medición. El eval la afina midiendo recall contra tokens inyectados.
3. **Disparador de guardado: patrón y tool.** Se detecta por patrón
   (`recuerda`, `no olvides`, `guarda esto`, `/save`) y además se expone
   `save_memory` para que el agente guarde por decisión propia.
4. **Confirmación visible** de una línea cuando se graba una memoria.

## 11. Secuencia

1. Harness de eval con ~50 casos; baseline contra supermemory.
2. Migración de las 171 a SQLite, tal cual, con tags por regex.
3. Binario con BM25 solamente. Medir.
4. Agregar la capa vectorial e híbrida **solo si supera a BM25 en el eval de
   50 casos** (D7). La medición preliminar sobre 390 documentos dice que
   podría no superarlo; si no lo hace, el sistema se queda en el paso 3 y se
   ahorran 220 MB de modelo y toda la dependencia de ONNX.
5. Clasificador de tags solo si el eval lo justifica.

Cada escalón se gana con un número.
