# Los documentos a inglés — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** El README, `AGENTS.md` y `docs/` quedan en inglés, y lo que
describen vuelve a coincidir con lo que el binario hace.

**Architecture:** Prosa, no código. Lo único que se construye es un guardia
que falla cuando un documento se separa del programa, que es el problema que
este paso descubre y no solo el que arregla.

**Tech Stack:** Markdown y un test de Rust.

**Spec:** `docs/superpowers/specs/2026-11-02-localizacion-design.md`, sección
final.

## Global Constraints

- **Traducir no es reescribir.** Los documentos explican decisiones reales y
  el porqué de cada una; esa es su razón de existir. Se traducen enteros, sin
  resumir ni podar.
- Los bloques de código, rutas, nombres de comando, banderas y salidas de
  ejemplo tienen que coincidir con lo que el binario hace **hoy**, no con lo
  que decía el documento.
- `docs/historia/` y `docs/superpowers/` se quedan en español: son registro
  de lo que pasó y material de proceso, no documentación de uso.

## Una decisión que corrige una anterior

El documento de diseño decía: `README.md` en inglés **con `README.es.md` al
lado**. Se descarta el segundo.

El motivo por el que anathem deja de ser bilingüe es que sostener dos idiomas
cuesta más que traducir una vez, y el README es el documento que más cambia
de todos: dejarlo en dos versiones es exactamente el costo que se quiso
evitar, en el peor lugar posible. Si más adelante alguien quiere el español,
agregarlo no necesita infraestructura ninguna.

## Review Focus

- Un documento que enseña una bandera que ya no existe. `--configurar-path` y
  `--conectar` pasaron a `--setup-path` y `--connect` en el paso anterior. →
  Task 1
- Un documento que promete una salida que el binario ya no imprime.
  `AGENTS.md` dice que `mem doctor` termina en «Todo en orden.» y hoy termina
  en «All good.» → Task 1
- Un bloque de código que no corre: una ruta que cambió, un subcomando que se
  renombró. → Task 1
- Español que sobrevive en un documento largo porque está en la mitad de
  abajo y nadie llegó. → Task 1
- El README documenta claves de `config.json`; ya hay un test que las valida
  (`docs_config_test.rs`) y tiene que seguir pasando. → Task 4

---

### Task 1: El guardia de los documentos

**Files:**
- Create: `tests/docs_english_test.rs`

Va **primero**, al revés que el guardia del paso anterior, que se escribió al
final y por eso no encontró lo que ya estaba roto. Escrito ahora, la lista de
lo que falla es la lista de trabajo de las tareas siguientes.

**Interfaces:**
- Produces: nada que otra tarea consuma. Las tareas 2-4 lo usan como criterio
  de terminado.

- [ ] **Step 1: Escribir el test**

Tres comprobaciones sobre `README.md`, `AGENTS.md` y `docs/*.md`, saltando
`docs/historia/` y `docs/superpowers/`:

```rust
/// Banderas y textos que el programa dejó de usar. Un documento que los
/// nombre está enseñando algo que no existe.
const GONE: &[(&str, &str)] = &[
    ("--configurar-path", "ahora --setup-path"),
    ("--conectar", "ahora --connect"),
    ("--sin-paquete", "ahora --no-pack"),
    ("--prefijo", "ahora --prefix"),
    ("--compilar", "ahora --build"),
    ("--etiqueta", "ahora --tag"),
    ("Todo en orden", "ahora «All good.»"),
    ("--desde", "ahora --since o --from"),
    ("--limite", "ahora --limit"),
    ("--todos", "ahora --all"),
    ("--fusionar", "ahora --merge"),
];
```

- `no_doc_teaches_something_that_no_longer_exists`: ningún documento contiene
  una entrada de `GONE`.
- `no_spanish_left_in_the_docs`: ninguna línea tiene acentos ni signos de
  apertura. Un documento es prosa entera, así que el criterio del guardia de
  `src/` —mirar solo cadenas— no aplica; acá alcanza con los acentos, porque
  cualquier párrafo en español lleva alguno.
- `every_documented_mem_command_exists`: por cada bloque ```bash, cada línea
  que empieza con `mem ` se compara contra los subcomandos que `mem --help`
  lista, y falla si nombra uno que no existe.

- [ ] **Step 2: Correrlo y anotar todo lo que falla** — es la lista de trabajo
- [ ] **Step 3: Commit del guardia, en rojo, con el porqué en el mensaje**

Se commitea fallando a propósito: el commit siguiente lo pone en verde, y el
par deja en el historial qué estaba roto y qué lo arregló.

---

### Task 2: `AGENTS.md`

**Files:**
- Modify: `AGENTS.md`

Es el más urgente de los tres: lo lee un agente que instala anathem para
alguien, y hoy le promete una salida que el binario ya no imprime. Un agente
que compruebe «termina en Todo en orden.» va a reportar que la instalación
falló cuando está perfecta.

- [ ] **Step 1: Traducir el documento entero**
- [ ] **Step 2: Corregir lo que quedó desactualizado**: el `Todo en orden.`,
      las banderas del instalador, y cualquier salida de ejemplo
- [ ] **Step 3: `cargo test --test docs_english_test`** — `AGENTS.md` deja de
      aparecer en los fallos
- [ ] **Step 4: Commit**

---

### Task 3: `docs/*.md`

**Files:**
- Modify: `docs/instalacion.md`, `docs/seguridad.md`, `docs/windows.md`,
  `docs/paquetes-de-vocabulario.md`, `docs/comparacion-con-engram.md`,
  `docs/formato-de-export.md`

Unas 870 líneas. `instalacion.md` es casi la mitad y el que más banderas
nombra.

- [ ] **Step 1: Traducir cada uno, entero**
- [ ] **Step 2: `cargo test --test docs_english_test`** — `docs/` deja de
      aparecer
- [ ] **Step 3: Commit**

---

### Task 4: `README.md`

**Files:**
- Modify: `README.md`

453 líneas, y es la puerta de entrada: es lo que alguien lee antes de decidir
si instala esto. Va último para que las decisiones de vocabulario ya estén
tomadas por los documentos anteriores y el README las siga en vez de
inventarlas.

- [ ] **Step 1: Traducir el documento entero**
- [ ] **Step 2: `cargo test`** — el guardia entero en verde, y
      `docs_config_test` sigue pasando: el README documenta claves de
      `config.json` y ese test las valida de verdad
- [ ] **Step 3: Un recorrido real**: correr los comandos de los bloques
      ```bash del README contra una base temporal y comprobar que la salida
      se parece a la documentada
- [ ] **Step 4: Commit**
