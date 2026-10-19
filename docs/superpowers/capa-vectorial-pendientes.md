# Capa vectorial: lo que quedó pendiente

Deuda anotada durante la implementación de la capa vectorial y diferida a
propósito: nada de esto es un defecto de corrección, y ninguno bloqueaba
integrar. Está acá para que no se pierda.

## Rendimiento, cuando el corpus crezca

**`buscar` aloca dos vectores por fila, en cada consulta.** `src/vecstore.rs`
copia el blob con `r.get::<_, Vec<u8>>` y después `de_blob` aloca un
`Vec<f32>`. A 100.000 fragmentos son 200.000 asignaciones y unos 200 MB de
copia **por búsqueda**, que probablemente dominen los 25 millones de
operaciones de punto flotante que el diseño presupuestó. Se elimina con
`row.get_ref(2)?.as_blob()?` y el producto punto sobre el slice prestado, sin
cambiar nada más.

**Hasta 200 consultas de una fila por búsqueda para devolver 10 resultados.**
`src/search.rs` arma el `SearchHit` completo de cada candidato vectorial antes
de la fusión, del agrupamiento por fijadas y del presupuesto, y solo
sobreviven `q.limit`. Diferir `hit_de_chunk` al bucle final elimina unos 190
viajes a la base por búsqueda.

**Un vector ocupa 1 KB por fragmento.** A partir de unos 100.000 fragmentos
conviene guardarlos en int8, como ya hace la tabla del paquete.

## Duplicación que va a costar un bug

**Los filtros de la consulta están escritos dos veces.** `src/search.rs` y
`src/vecstore.rs` construyen por separado el mismo WHERE de proyecto y tags,
cada uno con su numeración `?N` hecha a mano. Hoy coinciden. El día que se
agregue un filtro a `Query` y se actualice uno solo, el lado vectorial va a
devolver memorias que el léxico filtra, y ningún test lo va a ver.

## Diagnóstico y mensajes

- `mem eval` etiqueta sus resultados como `anathem bm25` aunque la corrida sea
  híbrida. El JSON archivado es justamente la evidencia contra la que se
  comparará el próximo cambio de ranking.
- La descripción de la tool MCP `search_memory` explica la capa vectorial a
  todo el mundo, tenga paquete o no. Podría depender de si hay uno activo.
- `config::cargar` usa `deny_unknown_fields`, así que una clave de más en
  `config.json` tumba `mem search` incluso sin paquete instalado. Es el único
  punto donde la capa vectorial puede romperle algo a quien no la usa.
- `mem model remove` ignora el error de `remove_file` y anuncia el paquete
  quitado aunque queden 22 MB en disco.

## Cobertura de pruebas

- `no_repite_documentos` en `tests/fusion_test.rs` es tautológico con una
  implementación basada en `HashMap`; se conserva como guarda ante un cambio
  de enfoque.
- `instalar_rechaza_un_archivo_que_no_es_paquete` solo afirma que el comando
  falla, cosa que también haría un subcomando inexistente. Convendría afirmar
  además que `paquetes/` quedó vacío.
- `cambiar_de_paquete_rehace_el_indice_en_vez_de_mezclar` fija el
  comportamiento de `model install`, no el de la guarda de identidad. La
  guarda la ejercitan los otros dos tests de incompatibilidad.

## Bordes conocidos y aceptados

- `de_blob` descarta en silencio los bytes sobrantes si el blob no es múltiplo
  de 4. Solo alcanzable con una base corrupta.
- `hit.score < 0.0` decide si un resultado tiene bm25, apoyándose en que FTS5
  nunca devuelve exactamente 0 para un acierto real. Funciona, pero es una
  suposición implícita; el olor de fondo es que `SearchHit.score` use 0,0 como
  centinela.
- Una instalación interrumpida antes del rename cuesta un reindexado completo.
  Distinguir ese caso exigiría el sha del archivo, que es el hash de 22 MB que
  el diseño decidió no pagar en cada arranque.
- La marca `pack_instalando` puede quedar huérfana si un `install` muere sin
  que hubiera paquete previo. Es inocua: nadie la lee y el siguiente `install`
  la sobrescribe. Ignorarla es lo que preserva la promesa de que sin paquete
  no cambia nada.
- El generador renumera `model.vocab` pero deja intactos los ids de
  `added_tokens`. Uno que caiga dentro del rango podado apuntaría a la fila de
  otra pieza; solo se alcanza con texto que contenga literalmente la cadena del
  token especial.

## Decisiones de alcance, no deuda

- `supersede::candidates` busca casi-duplicados solo con bm25. Corre
  inmediatamente después de un insert, cuando la memoria nueva puede no estar
  embebida todavía. Buscar duplicados por vector probablemente sea mejor, pero
  es un cambio de comportamiento que merece su propia decisión.
- El paquete se instala desde un archivo local. La descarga por HTTP haría
  falta para que un tercero lo instale sin bajarlo a mano primero.
- **Dónde se hospedan los paquetes sigue sin decidirse.** Sin eso,
  `mem model install` solo le sirve a quien genere el paquete en su propia
  máquina.

## Portabilidad: Windows no anda, macOS probablemente sí

Verificado leyendo el código; solo Linux está probado de verdad. El código de
producción no usa ninguna API de `std::os::unix`, así que los problemas son de
semántica del sistema operativo, no de imports.

**Windows, cuatro bloqueos concretos:**

1. `src/main.rs` resuelve el directorio del usuario con `std::env::var_os("HOME")`.
   En Windows esa variable normalmente no existe — es `USERPROFILE` — así que
   la ruta por defecto `~/.anathem/mem.db` falla con "HOME no está definido".
2. `std::fs::rename` en Windows **falla si el destino existe**, mientras que en
   POSIX lo reemplaza de forma atómica. `src/paquete.rs` renombra sobre el
   destino al instalar, que es exactamente el caso de actualizar un paquete ya
   instalado.
3. Windows no deja renombrar ni borrar un archivo que tenga un mapeo de memoria
   abierto. La reconciliación abre el `Pack` y después renombra ese mismo
   archivo.
4. `tests/vecstore_test.rs` usa `std::os::unix::fs::PermissionsExt`, así que la
   batería ni siquiera compila.

Los cuatro son arreglos chicos —`USERPROFILE` como alternativa, borrar antes de
renombrar o un helper de reemplazo, soltar el mapeo antes del renombrado, y un
`#[cfg(unix)]` en el test— pero ninguno está hecho ni probado.

**macOS:** no hay nada que sugiera que falle. Mismas semánticas de POSIX,
`HOME` definido, y las dependencias en C (SQLite de `rusqlite` con `bundled`,
oniguruma de `tokenizers`) compilan con las herramientas de línea de comandos
de Xcode. No está probado: no hay Mac donde correrlo.

## El costo en RAM es el tokenizador, no la tabla

Medido sobre el servidor MCP en uso, con el paquete `en-es` instalado:

| momento | RSS |
|---|---|
| servidor arrancado, con paquete instalado | 7 MB |
| tras la primera búsqueda | 64 MB |

El costo dominante no es la tabla de embeddings —que se mapea y se comparte
entre procesos— sino las estructuras que `tokenizers` arma al parsear el
`tokenizer.json` de 76.363 piezas. Ese heap es privado de cada proceso.

**Mitigado con carga perezosa.** El paquete se abre en el primer uso y no al
arrancar, así que una sesión que nunca consulta la memoria se queda en 7 MB.
Con ocho sesiones abiertas y pocas usándola, el total real bajó de ~430 MB a
unas decenas.

Lo que queda pendiente, y ahora vale bastante menos: hacer el tokenizador
mapeable desde el propio paquete, para que varias sesiones que **sí** buscan
compartan esas estructuras en vez de construir cada una las suyas. Bajaría el
costo de N sesiones activas de 57·N MB a 57 MB. El precio es reimplementar la
segmentación Unigram —unas doscientas líneas de Viterbi— y la ganancia solo
aparece con varias sesiones buscando a la vez. Tiene un beneficio lateral que
puede inclinar la balanza el día que importe: elimina la dependencia de
oniguruma, que es lo que obliga a un toolchain de C y complica Windows.

## `mem model remove` destruye el archivo del paquete

No solo borra los vectores y el registro: borra el `.anathem-pack` de
`~/.anathem/paquetes/`. Si era la única copia, reinstalar exige volver a
bajarla. Peor: invita a la secuencia `remove` seguido de `install` desde la
ruta instalada, que falla porque el archivo ya no está — pasó durante el
desarrollo y dejó la capa vectorial apagada sin que nadie lo notara hasta
revisar `model status`.

Arreglos posibles, ninguno hecho: que `remove` conserve el archivo y solo
desregistre; que avise que lo va a borrar; o que `install` detecte que el
origen es la ruta instalada y lo diga con claridad.

