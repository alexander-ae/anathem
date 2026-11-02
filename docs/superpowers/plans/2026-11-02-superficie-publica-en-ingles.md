# Normalizar la superficie pública a inglés — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dejar las claves del `config.json` y los nombres de las banderas en
inglés, sin romper archivos ni scripts existentes, y arreglar de paso los
mensajes de error que nombran claves inexistentes.

**Architecture:** Nada de infraestructura nueva. `serde(alias)` para las
claves viejas del config y `clap(alias, hide)` para las banderas viejas: en
los dos casos la forma anterior sigue funcionando y deja de aparecer en la
ayuda. Los mensajes de error pasan a nombrar la clave real.

**Tech Stack:** Rust, serde, clap 4, rusqlite. Sin dependencias nuevas.

**Spec:** `docs/superpowers/specs/2026-11-02-localizacion-design.md`

## Global Constraints

- Sin dependencias nuevas. El proyecto rechaza agregarlas por principio
  (`src/update.rs`).
- `#[serde(deny_unknown_fields)]` en `Config` **se conserva**: la detección de
  claves mal escritas es deliberada.
- Todo `config.json` escrito antes de este cambio tiene que seguir cargando
  sin aviso ni error.
- Todo script que use las banderas viejas tiene que seguir funcionando.
- Los comentarios nuevos en código van en inglés, como el resto de `src/`.
  Los mensajes al usuario siguen en español en este plan: traducirlos es el
  plan de localización, no este.

## Review Focus

- Un `config.json` con la clave vieja (`traspaso_items`) carga igual. → Task 2
- Un `config.json` con la clave vieja **y** la nueva a la vez es un error
  claro, no un valor elegido en silencio. → Task 2
- Un `config.json` con una clave inventada se sigue rechazando: el alias no
  puede haber aflojado `deny_unknown_fields`. → Task 2
- `mem recent --desde 6h --limite 3` sigue andando, y `--desde` no aparece en
  `mem recent --help`. → Task 3
- `--merge` no queda declarado dos veces: `Import` ya tiene un campo `merge`
  cuya bandera larga es `--fusionar`. → Task 3

---

### Task 1: Los errores del config nombran la clave real

**Files:**
- Modify: `src/config.rs:128-139`
- Modify: `src/doctor.rs:634-637`
- Test: `tests/config_test.rs:56-76`, `tests/doctor_test.rs:163-176`

**Interfaces:**
- Consumes: nada.
- Produces: nada que otra tarea use. Es un arreglo cerrado.

- [ ] **Step 1: Corregir los dos tests que fijan el bug**

En `tests/config_test.rs`, los tests
`candidates_at_zero_is_rejected_naming_file_and_field` y
`excerpt_at_zero_is_rejected_naming_file_and_field` escriben
`{"candidates": 0}` y `{"excerpt": 0}` —las claves reales— pero exigen que el
error diga `candidatos` y `extracto`. Cambiar esas dos aserciones a
`e.contains("candidates")` y `e.contains("excerpt")`.

Agregar un tercero para la clave que faltaba:

```rust
#[test]
fn bm25_weight_negative_is_rejected_naming_the_real_key() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": -1.0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("bm25_weight"), "el error debe nombrar la clave real: {e}");
}
```

En `tests/doctor_test.rs`, cambiar `candidatos=7` → `candidates=7` y
`extracto=120` → `excerpt=120` (líneas 173 y 176), y `candidatos` →
`candidates` en la 163.

- [ ] **Step 2: Correrlos y ver que fallan**

Run: `cargo test --test config_test --test doctor_test`
Expected: FAIL. Los de config dicen que el error contiene `candidatos` y se
esperaba `candidates`.

- [ ] **Step 3: Corregir los mensajes**

En `src/config.rs`, los tres `bail!` pasan a nombrar `bm25_weight`,
`candidates` y `excerpt`. En `src/doctor.rs:635`, la cadena `"peso_bm25={:?},
candidatos={}, extracto={}"` pasa a `"bm25_weight={:?}, candidates={},
excerpt={}"`.

- [ ] **Step 4: Correrlos y ver que pasan**

Run: `cargo test --test config_test --test doctor_test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/doctor.rs tests/config_test.rs tests/doctor_test.rs
git commit -m "fix: los errores de config.json nombran la clave que hay que arreglar"
```

---

### Task 2: Claves del config en inglés, con alias

**Files:**
- Modify: `src/config.rs:54-74`
- Test: `tests/config_test.rs`

**Interfaces:**
- Consumes: Task 1 (los mensajes ya nombran la clave real).
- Produces: `Config` con las claves JSON `handoff_items`, `handoff_window` y
  `release_pack_after`. Los campos de Rust no cambian de nombre.

- [ ] **Step 1: Escribir los tests**

```rust
#[test]
fn the_old_spanish_keys_still_load() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"traspaso_items": 3, "traspaso_ventana": "6h", "soltar_paquete_tras": 60}"#).unwrap();
    let c = config::load(d.path()).unwrap();
    assert_eq!(c.handoff_items, 3);
    assert_eq!(c.handoff_window, "6h");
    assert_eq!(c.release_pack_after, 60);
}

#[test]
fn the_new_english_keys_load() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"handoff_items": 3, "handoff_window": "6h", "release_pack_after": 60}"#).unwrap();
    let c = config::load(d.path()).unwrap();
    assert_eq!(c.handoff_items, 3);
}

#[test]
fn the_old_and_new_key_together_is_an_error() {
    // Elegir una de las dos en silencio dejaría al usuario creyendo que
    // aplicó la que no se aplicó.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"traspaso_items": 3, "handoff_items": 9}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}

#[test]
fn an_unknown_key_is_still_rejected() {
    // El alias no puede haber aflojado deny_unknown_fields.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"inventada": 1}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}
```

- [ ] **Step 2: Correrlos y ver cuáles fallan**

Run: `cargo test --test config_test`
Expected: `the_new_english_keys_load` FAIL (hoy la clave es `traspaso_items`).
Los otros tres pasan ya.

- [ ] **Step 3: Cambiar `rename` por `alias` en los tres campos**

En `src/config.rs`, los tres `#[serde(rename = "...")]` de `handoff_items`,
`handoff_window` y `release_pack_after` pasan a
`#[serde(alias = "traspaso_items")]` y equivalentes. Al quitar el `rename`, la
clave canónica vuelve a ser el nombre del campo de Rust, que ya está en
inglés. Dejar un comentario diciendo que el alias es compatibilidad con
archivos anteriores a 0.2.1 y por qué no se quita.

- [ ] **Step 4: Correr toda la suite**

Run: `cargo test`
Expected: PASS. Si algún test escribía las claves viejas, sigue andando por el
alias.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "refactor: las claves del config.json pasan a inglés, con alias para las viejas"
```

---

### Task 3: Banderas en inglés, con alias oculto

**Files:**
- Modify: `src/main.rs:124-145`
- Test: `tests/cli_flags_test.rs` (crear)

**Interfaces:**
- Consumes: nada.
- Produces: las banderas `--all`, `--from`, `--limit` y `--merge`, con
  `--todos`, `--desde`, `--limite` y `--fusionar` como alias ocultos.

**Cuidado:** `Import` ya tiene un campo llamado `merge` cuya bandera larga es
`--fusionar`. Al renombrarla a `--merge` hay que verificar que no choque con
otra declaración.

- [ ] **Step 1: Escribir el test**

```rust
//! Las banderas viejas en español siguen funcionando y ya no se muestran.

fn mem(db: &std::path::Path, args: &[&str]) -> (String, bool) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), o.status.success())
}

#[test]
fn the_new_english_flags_work() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    mem(&db, &["init"]);
    assert!(mem(&db, &["recent", "--from", "6h", "--limit", "3"]).1);
    assert!(mem(&db, &["recent", "--all"]).1);
}

#[test]
fn the_old_spanish_flags_still_work() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    mem(&db, &["init"]);
    assert!(mem(&db, &["recent", "--desde", "6h", "--limite", "3"]).1);
    assert!(mem(&db, &["recent", "--todos"]).1);
}

#[test]
fn the_old_flags_are_hidden_from_help() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["recent", "--help"]);
    assert!(ok);
    assert!(help.contains("--from"), "la nueva tiene que aparecer: {help}");
    assert!(!help.contains("--desde"), "la vieja no: {help}");
}
```

- [ ] **Step 2: Correrlos y ver que fallan**

Run: `cargo test --test cli_flags_test`
Expected: FAIL. `--from`, `--limit` y `--all` no existen todavía.

- [ ] **Step 3: Renombrar las cuatro banderas**

En `src/main.rs`, para cada una: cambiar `long = "<español>"` por el nombre
en inglés y agregar `alias = "<español>"`. En clap 4, `alias` **no** aparece
en la ayuda y `visible_alias` sí, así que `alias` es exactamente lo que se
quiere y no hace falta ningún `hide`. Queda, por ejemplo:

```rust
#[arg(long = "from", alias = "desde", value_name = "FROM", default_value = "24h")]
```

Actualizar también `value_name` (`DESDE` → `FROM`, `LIMITE` → `LIMIT`) y el
`conflicts_with = "project"` de `--all` si se nombra por el identificador de
la bandera.

- [ ] **Step 4: Correr toda la suite**

Run: `cargo test`
Expected: PASS. Atender en particular `tests/recent_test.rs` y
`tests/import_test.rs`, que invocan estas banderas.

- [ ] **Step 5: Actualizar los docs que las nombran**

Run: `grep -rn -- '--desde\|--limite\|--todos\|--fusionar' docs/ README.md AGENTS.md`
Cambiar las ocurrencias a la forma nueva. No traducir el texto alrededor:
eso es el plan de localización.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/cli_flags_test.rs docs/ README.md AGENTS.md
git commit -m "refactor: las banderas pasan a inglés, con las viejas como alias oculto"
```

---

## Los planes que siguen

Este plan es el primero de cuatro. Los otros tres se escriben cuando les toque;
cada uno produce software funcionando por su cuenta y se puede publicar solo:

1. **Superficie pública a inglés** — este.
2. **`mcp.rs` a inglés.** Descripciones de tools, `inputSchema`, resultados y
   el texto de traspaso. Incluye el bug del `replace_file` y el cambio de
   `[tema:` a `[topic:` en lockstep con `topic_test`.
3. **Infraestructura i18n y catálogo del CLI.** El grande: el enum `Msg`, la
   cascada con barrido de `env::args()`, el `Lang` en los productores
   compartidos, la mutación del `Command` de clap, y el `language` que escribe
   el instalador.
4. **Docs y README.** `README.md` en inglés con `README.es.md` al lado,
   `docs/` a inglés, el español que quede en neutro. Sin código.
