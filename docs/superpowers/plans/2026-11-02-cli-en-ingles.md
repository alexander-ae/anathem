# El CLI a inglés, y `Lang` se borra — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** No queda español en nada que anathem imprima o muestre, y el enum
`Lang` desaparece.

**Architecture:** Trabajo mecánico, sin decisiones de diseño pendientes: el
documento de diseño ya las tomó todas. Empieza por borrar `Lang` quedándose
con la rama inglesa, lo cual traduce de una sola vez los cinco productores
compartidos. Después, archivo por archivo. Termina con un test que falla si
alguien vuelve a escribir español en una cadena.

**Tech Stack:** Rust. Sin dependencias nuevas; de hecho el trabajo quita
código.

**Spec:** `docs/superpowers/specs/2026-11-02-localizacion-design.md`, en
particular la sección final «La decisión que revierte a este documento».

## Global Constraints

- **El comportamiento no cambia, solo el idioma.** Ningún test puede
  relajarse: si uno afirmaba que un error nombra un id, tiene que seguir
  afirmándolo en inglés. Cambiar el idioma de una aserción está bien;
  borrarla o debilitarla, no.
- Lo que escribió el usuario nunca se toca: títulos, cuerpos, claves de tema,
  nombres de proyecto y de agente viajan como están.
- Los nombres de comandos, subcomandos y banderas ya están en inglés y no se
  tocan.
- Sin dependencias nuevas.

## Review Focus

- Un mensaje mitad inglés mitad español, que es el modo de falla que este
  trabajo ya produjo dos veces: una capa traducida que envuelve otra que no.
  → Task 6
- `mem doctor` sigue saliendo con 0 / 1 / 2 según corresponda después de
  traducir sus 39 líneas de informe. → Task 3
- El texto de ayuda de clap sale bien formado: un doc comment traducido mal
  cortado deja la ayuda sin la primera línea. → Task 2
- Los acentos desaparecen de las cadenas pero no de los datos: una memoria
  guardada en español se sigue mostrando en español. → Task 6
- `mem update` sigue reconociendo una versión nueva: sus mensajes incluyen la
  comparación de versiones. → Task 4

---

### Task 1: Borrar `Lang`, quedándose con el inglés

**Files:**
- Delete: `src/lang.rs`, `tests/lang_test.rs`
- Modify: `src/lib.rs`, `src/recent.rs`, `src/ingest.rs`, `src/mcp.rs`, `src/main.rs`
- Test: `tests/topic_test.rs`, `tests/recent_test.rs`

**Interfaces:**
- Produces: `duration_in_seconds(&str)`, `Saved::describe(&self)`,
  `format_items(&[RecentItem], bool)`, `handoff_instructions(..., Option<&str>)`
  y `save_with_topic_by(..., Option<&str>)` — todas sin el parámetro `lang`,
  como antes del plan 2, pero con el texto inglés.

Borrar el enum traduce de una vez los cinco productores compartidos: en cada
`match lang` o `lang.pick(en, es)` se queda la rama inglesa.

- [ ] **Step 1: Actualizar los tests del CLI que afirman en español**

`tests/topic_test.rs` y `tests/recent_test.rs` tienen aserciones sobre salida
de terminal en español (`reemplaza a la memoria`, `[tema:`, `esta herramienta`,
`sin memorias en esa ventana`). Pasan al texto inglés que ya existe en la rama
`En` de cada función. **Las aserciones siguen afirmando lo mismo**, solo
cambia el idioma del literal.

- [ ] **Step 2: Correr y ver que fallan**

Run: `cargo test --test topic_test --test recent_test`
Expected: FAIL, porque el CLI todavía imprime español.

- [ ] **Step 3: Borrar `Lang` y colapsar las firmas**

Quitar `pub mod lang;`, borrar `src/lang.rs` y `tests/lang_test.rs`, y en cada
sitio que usa `lang` quedarse con el inglés y sacar el parámetro. Los
llamadores pierden el argumento.

- [ ] **Step 4: Correr la suite entera**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: Lang se borra; los productores compartidos quedan en inglés"
```

---

### Task 2: `main.rs` — el CLI y la ayuda de clap

**Files:**
- Modify: `src/main.rs`
- Test: `tests/first_run_test.rs`, `tests/cli_flags_test.rs`, y cualquiera que afirme sobre stdout del binario

**Interfaces:**
- Consumes: Task 1.

Son unos 15 mensajes y **102 doc comments** que clap convierte en ayuda. Los
doc comments son la mitad larga del trabajo y no tienen test propio: el de
`cli_flags_test` solo mira que las banderas nuevas aparezcan.

- [ ] **Step 1: Escribir el test de la ayuda**

```rust
#[test]
fn the_help_is_english_and_well_formed() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["--help"]);
    assert!(ok, "{help}");
    // La primera línea de cada doc comment es el resumen del subcomando: si
    // alguno quedó mal cortado al traducir, desaparece de la ayuda.
    for sub in ["save", "search", "recent", "doctor", "export", "import", "model"] {
        assert!(help.contains(sub), "falta el subcomando {sub}: {help}");
    }
    for palabra in ["memoria", "búsqueda", "proyecto", "guarda", "archivo"] {
        assert!(!help.contains(palabra), "queda español en la ayuda: «{palabra}»");
    }
}
```

- [ ] **Step 2: Correrlo y ver que falla**

Run: `cargo test --test cli_flags_test the_help_is_english`
Expected: FAIL por el español.

- [ ] **Step 3: Traducir `main.rs`**

Los doc comments de `Cli`, de cada variante de `Cmd` y de cada campo, más los
`println!`/`eprintln!`/`bail!` del cuerpo. No tocar nombres de banderas ni de
subcomandos.

- [ ] **Step 4: Correr la suite entera**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/
git commit -m "refactor: el CLI y su ayuda, en inglés"
```

---

### Task 3: `doctor.rs` — el informe

**Files:**
- Modify: `src/doctor.rs`
- Test: `tests/doctor_test.rs`

39 líneas de informe (`bien`/`mal`/`warning`) más sus mensajes. `doctor_test`
tiene 28 tests que afirman sobre ese texto, así que es el archivo con más
aserciones por línea traducida.

- [ ] **Step 1: Actualizar las aserciones de `doctor_test` al inglés**
- [ ] **Step 2: Correr y ver que fallan** — `cargo test --test doctor_test`, FAIL
- [ ] **Step 3: Traducir `doctor.rs`**, incluido el `Todo en orden.` final y los textos de los tres códigos de salida
- [ ] **Step 4: Correr la suite entera** — PASS, y comprobar a mano que `mem doctor` sigue saliendo 0 en una base sana y 1 con problemas
- [ ] **Step 5: Commit**

---

### Task 4: El resto de `src/`

**Files:**
- Modify: `src/pack.rs`, `src/packstore.rs`, `src/tags.rs`, `src/ingest.rs`, `src/config.rs`, `src/update.rs`, `src/supersede.rs`, `src/migrate.rs`, `src/export.rs`, `src/import.rs`, `src/evalrun.rs`, `src/db.rs`, `src/permisos.rs`, `src/edit.rs`, `src/maint.rs`, `src/vecstore.rs`, `src/recent.rs`
- Test: el que corresponda a cada módulo

Unos 40 literales repartidos. Los de `pack.rs` y `packstore.rs` son los que
más tests tienen detrás (`pack_test`, `corrupt_pack_test`, `model_cli_test`),
y son errores que nombran archivo y causa: las aserciones existentes exigen
que sigan nombrándolos.

- [ ] **Step 1: Actualizar las aserciones en español de los tests de esos módulos**
- [ ] **Step 2: Correr y ver que fallan**
- [ ] **Step 3: Traducir módulo por módulo**
- [ ] **Step 4: Correr la suite entera** — PASS
- [ ] **Step 5: Commit**

---

### Task 5: `scripts/instalar.sh`

**Files:**
- Modify: `scripts/instalar.sh`, `scripts/publicar-mac.sh`

Toda la salida del instalador, su `uso()` y los nombres de sus banderas
(`--compilar`, `--etiqueta`, `--paquete`, `--sin-paquete`, `--configurar-path`,
`--conectar`, `--prefijo`, `--sin-verificar`). **Acá sí cambian nombres de
banderas**, al revés que en el CLI: estas son del script y nadie las tiene en
un alias.

- [ ] **Step 1: Traducir el script**, con las banderas viejas aceptadas en el `case` para no romper a quien las tenga escritas
- [ ] **Step 2: `sh -n scripts/instalar.sh`** y un recorrido real con `--sin-paquete` contra un prefijo temporal
- [ ] **Step 3: Commit**

---

### Task 6: El guardia que impide que vuelva

**Files:**
- Create: `tests/english_only_test.rs`

- [ ] **Step 1: Escribir el guardia**

```rust
//! No queda español en lo que anathem imprime. Es el modo de falla que este
//! trabajo produjo dos veces: una capa traducida envolviendo otra que no.

/// Palabras que no aparecen en inglés y sí en el español que había.
const DELATORAS: &[&str] = &[
    "memoria", "búsqueda", "guardar", "guardada", "archivo", "paquete",
    "clave", "proyecto", "ventana", "tema", "nombre de", "no se pudo",
];

#[test]
fn no_spanish_left_in_any_string_literal() {
    for f in std::fs::read_dir("src").unwrap() {
        let p = f.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            let texto = std::fs::read_to_string(&p).unwrap();
            for (n, linea) in texto.lines().enumerate() {
                let l = linea.trim_start();
                if l.starts_with("//") || l.starts_with("///") { continue; }
                let Some(ini) = linea.find('"') else { continue };
                let cadena = &linea[ini..];
                for d in DELATORAS {
                    assert!(
                        !cadena.to_lowercase().contains(d),
                        "{}:{} tiene «{d}» en una cadena: {linea}",
                        p.display(), n + 1
                    );
                }
            }
        }
    }
}
```

El test mira solo cadenas y saltea comentarios: los comentarios del
repositorio ya están en inglés por otra vía, y mezclar las dos cosas haría
fallar el guardia por razones que no son las suyas.

- [ ] **Step 2: Correrlo** — tiene que pasar si las tareas 1-4 están completas; si falla, señala lo que quedó
- [ ] **Step 3: Commit**
