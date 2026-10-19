# Revisión de `mem doctor`

Qué encontró la revisión de `mem doctor` y `mem --version`, y qué se arregló.
La revisión no encontró críticos; los importantes están abajo, todos
reproducidos antes de arreglarlos.

## Ronda de arreglos 1

Commits: `631da96`, `04b6953`, `e2a1192`, `55316f1`, `3444f8d`. `cargo test`: 251 pasan, 0 fallan (222 previos sin tocar + 29 de esta funcionalidad: 28 en `doctor_test.rs`, 1 en `build_commit_test.rs`).

- **Importante 1 (`631da96`)**: reproducido primero con un crate mínimo: con el `build.rs` anterior, tras `git pack-refs --all` y un commit nuevo el binario seguía diciendo el hash viejo. Arreglo: se observa también `logs/HEAD` (vía `git rev-parse --git-path`). Un hallazgo propio: la sugerencia sola no cubre el primer commit, porque con cero commits `logs/HEAD` no existe y omitir los archivos inexistentes (para no recompilar siempre) dejaba el commit congelado en `desconocido`; mientras no hay ningún commit se observan aunque falten. Test nuevo `tests/build_commit_test.rs`: copia el `build.rs` real a un repo temporal y verifica el commit sin commits, tras el primero, tras `pack-refs` y tras dos commits más; falla con el `build.rs` anterior. README aclara que el commit no refleja cambios sin commitear y que doctor no lo compara.
- **Importante 2 (`04b6953`)**: base inexistente pasa a Aviso; el resumen dice `Sin problemas (1 aviso)` y no `Todo en orden.`; código 0. Dos tests nuevos (el existente conserva sus afirmaciones y se agregó uno que fija el texto veraz y el símbolo `!`).
- **Importante 3 (`e2a1192`)**: se detectan las tablas ausentes (`chunk_vectors`, `meta`, por separado y juntas) y se reporta una línea Info, sin tropezar con ellas. Test nuevo que además comprueba que la próxima escritura la migra y el mensaje desaparece; falla sin el arreglo.
- **Menor: código de salida (`55316f1`)**: 0 sin problemas, 1 problemas, 2 doctor no pudo correr. `main` ahora atrapa el error de `ejecutar` y, solo para doctor, sale con 2; los demás comandos siguen en 1 (test lo afirma). Documentado en el módulo, `--help` y README.
- **Menor: FTS5 por ids (`3444f8d`)**: test con igual conteo y ids distintos (se saca el 1, se mete el 99); doc del módulo dice que FTS5 se comprueba por presencia y no por contenido.

Inquietudes: un `panic` de doctor sale con 101, no con 2 (no se instaló un hook); el test de `build.rs` invoca `cargo run --offline` y requiere git (se omite sin git); el mensaje de salida de errores de los demás comandos conserva el formato `Error: ...` de antes.
