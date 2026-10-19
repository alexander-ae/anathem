# anathem en Windows

**Estado: compila y pasa la batería en CI; falta probarlo a mano.** Cada push
compila con MSVC en un runner `windows-latest` y corre 249 tests (uno queda
ignorado: el de permisos POSIX, que en Windows no aplica), más un recorrido de
`init`, `save`, `search` y `doctor`. Antes de eso nadie lo había compilado
nunca en Windows.

Lo que el CI **no** puede cubrir está más abajo, en "Qué probar": el CI corre
todo bajo un solo shell, así que no compara la ruta por defecto entre
`cmd.exe` y Git Bash, que es el punto donde una precedencia mal puesta
dejaría dos bases de datos distintas sin avisar.

Para referencia: Linux pasa 251 y macOS 250 (el que falta lee `/proc`, que
solo existe en Linux).

## Qué hacía falta arreglar

El código de producción nunca usó APIs de `std::os::unix`. Los cuatro bloqueos
eran de semántica del sistema operativo:

| | problema | arreglo |
|---|---|---|
| 1 | `HOME` no existe en Windows, y la ruta por defecto `~/.anathem/mem.db` fallaba | se usa `USERPROFILE`, con precedencia sobre `HOME` solo en Windows |
| 2 | `std::fs::rename` **falla si el destino existe** en Windows, mientras que en POSIX lo reemplaza | helper `reemplazar` que aparta el destino, renombra, y recién entonces borra el apartado |
| 3 | Windows no deja renombrar un archivo con un mapeo de memoria abierto | `reconciliar` suelta el `Pack` antes de renombrar y lo reabre después |
| 4 | un test usaba `PermissionsExt`, así que la batería no compilaba | `#[cfg(unix)]` más un test compañero `#[cfg(windows)] #[ignore]` para que el conteo no mienta |

Sobre el punto 1 hay un detalle que no es obvio: en Git Bash y MSYS, `HOME` sí
existe, pero en formato posix (`/c/Users/alguien`). Para un binario **nativo**
de Windows esa no es una ruta de usuario válida — un `/` inicial sin letra de
unidad se resuelve contra la raíz de la unidad actual. Si `HOME` tuviera
precedencia, la misma persona corriendo el mismo binario desde `cmd.exe` y
desde Git Bash terminaría con **dos bases de datos distintas** sin ningún
aviso. Por eso en Windows manda `USERPROFILE`.

Los arreglos son **neutrales por plataforma** a propósito: usan `cfg!(windows)`
—una macro que evalúa a un booleano y deja compilar las dos ramas— en vez de
`#[cfg(windows)]`, que dejaría código sin compilar ni probar en Linux. El
booleano se pasa como parámetro, así que los tests ejercitan **las dos**
precedencias y los dos editores por defecto desde Linux. El único `#[cfg]` de
plataforma está en el test del punto 4, donde es estructuralmente inevitable.

## Compilar

Hace falta un toolchain de C, igual que en Linux: el proyecto compila SQLite
(`rusqlite` con `bundled`) y oniguruma (`tokenizers`) desde fuente.

    rustup toolchain install stable-x86_64-pc-windows-msvc
    cargo build --release
    cargo test

Con MSVC hacen falta las *Build Tools for Visual Studio* con la carga de
trabajo de C++. Si `cargo build` falla en `libsqlite3-sys` o en `onig_sys`, es
eso y no el código de anathem.

## Qué probar, en este orden

1. ~~**Que compile.**~~ Ya está: el CI compila con MSVC en cada push. Era lo
   único que no se podía verificar de ninguna forma desde Linux.
2. **La ruta por defecto.** Corré `mem init` desde `cmd.exe` y después desde
   Git Bash, y comprobá que las dos imprimen **la misma** ruta. Si difieren, la
   precedencia de variables está mal y vas a tener dos bases.
3. **`mem model install` sobre un paquete ya instalado.** Es el caso que en
   Windows fallaba con el `rename` original. Tiene que reinstalar sin error y
   dejar el archivo del paquete intacto y completo.
4. **El escenario crítico sin resolver, abajo.**
5. **`mem edit` sin `$EDITOR` definida.** Debería abrir `notepad`.

## Lo que NO está resuelto y hay que medir primero

**Reinstalar el paquete mientras el servidor MCP está corriendo.** El servidor
vive horas con el archivo del paquete mapeado en memoria. Si desde otra
terminal se corre `mem model install` del mismo paquete, el proceso que instala
suelta *su* `Pack`, pero no puede soltar el del servidor. En Windows, si el
mapeo del otro proceso bloquea el renombrado, la instalación fallaría — y ese
es un uso que el diseño declara soportar, no un caso de borde.

Depende de con qué banderas de compartición abre `memmap2` el archivo, cosa que
no se puede determinar sin ejecutarlo. **Es el primer experimento a correr**:
arrancá `mem serve`, y desde otra terminal reinstalá el mismo paquete. Si falla,
el arreglo probable es que el servidor suelte y reabra el mapeo al detectar el
cambio, que es lo que ya hace `reconciliar` dentro de un mismo proceso.

## Deuda menor conocida

`reemplazar` deja un archivo lateral —`{nombre}.anathem-pack.reemplazando-{pid}`—
si el proceso muere entre apartar el destino y terminar. Es basura inerte: se
verificó que ninguna función de producción enumera `paquetes/` por patrón, así
que no confunde a la resolución de rutas ni a la guarda de identidad del
paquete. Se puede borrar a mano sin consecuencias.
