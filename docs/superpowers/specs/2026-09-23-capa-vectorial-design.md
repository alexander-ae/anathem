# Capa vectorial opcional para búsqueda entre idiomas

**Estado:** diseño aprobado, sin implementar.
**Mediciones que lo sustentan:** `docs/superpowers/medicion-multiidioma.md`,
`docs/superpowers/medicion-corpus-real.md`.
**Procedimiento de paquetes:** `docs/paquetes-de-vocabulario.md`.

## Problema

anathem busca con BM25 sobre FTS5. Una memoria escrita en español no se
encuentra preguntando en inglés: sobre el corpus real, recall@5 cae de 66 % a
34 %. BM25 no comprende, solo engancha identificadores y préstamos que
sobreviven a la traducción.

Un modelo estático destilado sube ese 34 % a 62 %, y fusionado con BM25 a
74 %. Cuesta un paquete de 22,5 MB que el usuario descarga una sola vez y solo
si lo necesita.

## Alcance

Entra: capa vectorial opcional, fusión con BM25, comandos para instalar y
quitar paquetes, reindexado. No entra: descarga por HTTP (el paquete se
instala desde un archivo local), generación de paquetes desde el binario (eso
lo hace `tools/construir_paquete.py`), y reranking posterior.

## Principio rector

**Sin paquete instalado, anathem se comporta exactamente como hoy.** La capa
vectorial no puede volverse un requisito encubierto: ni en el arranque, ni en
la migración, ni en la búsqueda. Quien no la instale no paga nada, ni en disco
ni en latencia ni en mensajes de advertencia repetidos.

## Componentes

### `src/pack.rs` — el paquete

Encapsula todo lo que sabe de embeddings. Expone:

    Pack::abrir(ruta) -> Result<Pack>       // valida magic, versión, dimensiones
    Pack::nombre(&self) -> &str
    Pack::dims(&self) -> usize
    Pack::embeber(&self, texto: &str) -> Vec<f32>   // normalizado

Dependencias nuevas: `tokenizers` y `memmap2`. Sin ONNX, sin torch, sin
servidor. La tabla se mapea en memoria y las filas se descuantizan al vuelo;
una fila son 256 bytes.

Nada fuera de este módulo sabe que existe un tokenizador o una cuantización.

### `src/vecstore.rs` — la persistencia

Guarda los vectores, los sincroniza con los fragmentos y los recorre por
coseno. Está separado de `pack.rs` porque leer el formato del modelo y
persistir vectores en SQLite son responsabilidades distintas: la primera
cambia si cambia el formato del paquete, la segunda si cambia el esquema.

### `src/fusion.rs` — el orden

Función pura, sin base de datos y sin modelo:

    rrf(bm25: &[i64], vectorial: &[i64], peso_bm25: f64) -> Vec<i64>

Recibe dos listas de `memory_id` ya ordenadas y devuelve una. Se prueba sola,
con listas escritas a mano. Está separada de `search.rs` justamente para que
la decisión de orden —que es donde vive el compromiso entre monolingüe y
cruzado— sea auditable sin montar un corpus.

Constante de RRF: k = 60. Peso de BM25 por defecto **0,25**, configurable en
`<dir_db>/config.json`. La medición sobre el corpus real: con 0,25 el cruzado
va de 58 % a 74 % y el monolingüe baja de 68 % a 64 %. No existe un peso que
gane en ambas direcciones; se elige 0,25 porque el caso de uso declarado es
multiidioma y la asimetría es de 16 puntos contra 4.

### Esquema

Aditivo, con `create ... if not exists` como el resto:

    create table if not exists chunk_vectors (
      chunk_id integer primary key references chunks(id) on delete cascade,
      vec      blob not null          -- f32 LE x dims, ya normalizado
    );

    create table if not exists meta (
      clave text primary key,
      valor text not null
    );

Claves de `meta`: `pack_nombre`, `pack_sha256`, `pack_dims`, `pack_instalando`.

`pack_instalando` se agregó después de la implementación y cierra una
ventana que esta especificación no había previsto. `install` pone el archivo
en su lugar con un `rename`, que es atómico, pero el registro que lo
describe se escribe en el paso siguiente: entre los dos,
`paquetes/{nombre}.anathem-pack` es el build nuevo y `meta` todavía describe
al viejo. Un corte de luz o un SIGKILL ahí deja esa mentira en disco para
siempre.

Ninguna de las tres claves originales la detecta. Cuando el build nuevo se
llama igual y tiene las mismas dimensiones que el viejo —actualizar el build
de `en-es`, el motivo más común para reinstalar— la guarda de la tabla de
errores de más abajo no dispara, y el sha registrado se descarta a propósito
para no hashear 22 MB en cada arranque. El resultado son consultas embebidas
con el paquete nuevo contra vectores del viejo, indefinidamente, con
`model status` informando cobertura completa: resultados plausibles y
equivocados, que es el modo de fallo que esta especificación prohíbe.

`pack_instalando` guarda el nombre del paquete que se está por reemplazar.
Se escribe **antes** del rename y la borra `adoptar_pack` en la misma
transacción en la que registra el paquete nuevo, así que su existencia
equivale exactamente a "el archivo en disco puede no ser el que `meta`
describe". Al resolver el paquete, si la marca nombra al paquete registrado
se reindexa antes de buscar; si nombra a otro, el archivo del registrado no
lo tocó nadie y la marca se limpia. No acorta la ventana: la declara.

Un vector son 1 KB por fragmento. El corpus de referencia (1.024 fragmentos)
ocupa 1 MB. A partir de unos 100.000 fragmentos conviene guardarlos en int8;
queda anotado, no se implementa ahora.

### Búsqueda vectorial: fuerza bruta

Sin `sqlite-vec` ni índice aproximado. Un producto punto contra 100.000
fragmentos son 25 millones de operaciones, unos pocos milisegundos en Rust, y
evita una extensión compilada que rompería la promesa de un solo binario.
Revisar por encima del medio millón de fragmentos.

## Flujo

**Al indexar** (`mem save`, `mem migrate`): después de insertar los
fragmentos, si hay paquete instalado se embebe cada uno y se guarda en
`chunk_vectors`. Sin paquete, no pasa nada.

**Al buscar** (`search::run_explained`):

1. Candidatos por BM25 — el código actual, sin cambios.
2. Si hay paquete: se embebe la consulta y se recorren los vectores de los
   fragmentos que pasan los mismos filtros (estado activo, proyecto, tags).
   Top 200 por coseno, colapsado a memoria por máximo.
3. `fusion::rrf` combina los dos órdenes.
4. Las memorias `pinned` siguen primero, como hoy. La fusión ordena dentro de
   cada grupo, no lo atraviesa.
5. Se aplica el presupuesto de tokens y se devuelve.

Sin paquete, los pasos 2 y 3 se saltean y el resultado es idéntico al actual.

## Comandos

    mem model install <archivo.anathem-pack>   # verifica sha256, instala, reindexa
    mem model status                            # paquete activo y cobertura
    mem model remove                            # borra paquete y vectores
    mem reindex                                 # rehace todos los vectores

`install` copia el paquete a `<dir_db>/paquetes/` y anota nombre y sha256 en
`meta`. El sha256 **se verifica solo al instalar**: hashear 22 MB en cada
arranque contradiría el principio de no penalizar a quien no usa la capa. En
cada apertura se validan magic, versión, dimensiones y que el nombre embebido
coincida con `pack_nombre`, que es lo que detecta el caso real de haber
cambiado de paquete. Sustituir a mano el archivo por otro del mismo nombre y
dimensiones queda fuera de alcance.

## Errores

Cada uno tiene una respuesta distinta y deliberada:

| situación | respuesta |
|---|---|
| archivo corrupto, magic o versión inesperados | `install` falla ruidosamente; no se instala nada |
| sha256 no coincide con el `.sha256` | `install` falla; el paquete se descarta |
| el nombre del paquete abierto difiere de `pack_nombre` | se reindexa antes de buscar; **nunca se mezclan vectores de paquetes distintos** |
| `pack_dims` no coincide con los vectores guardados | se reindexa |
| el archivo del paquete desapareció | se avisa una vez y se busca solo con BM25; buscar nunca falla por esto |
| fragmentos sin vector (creados sin paquete) | BM25 igual los cubre; `model status` informa la cobertura para que el hueco sea visible |
| `pack_instalando` quedó puesto nombrando al paquete registrado | una instalación murió entre el rename y el registro: se reindexa antes de buscar, aunque el nombre y las dimensiones coincidan |
| el paquete registrado cambia mientras otro proceso está embebiendo | el lote en curso se abandona sin escribirlo y se avisa; quedan fragmentos sin vector, nunca vectores de dos paquetes |

La regla de fondo: **degradar a BM25 es aceptable y debe ser silencioso salvo
un aviso; mezclar vectores incompatibles no es aceptable nunca.** Un índice
mezclado devuelve resultados plausibles y equivocados, que es el peor modo de
fallo posible para una herramienta de memoria.

## Pruebas

- **Carga del paquete**: un paquete mínimo de fixture; se verifican nombre,
  dimensiones y cantidad de tokens.
- **Determinismo**: el mismo texto produce el mismo vector entre corridas.
- **Fusión**: listas escritas a mano; se comprueba que el peso 0,25 y el 1,0
  producen los órdenes esperados. Sin base y sin modelo.
- **Incompatibilidad**: se indexa con el paquete A, se instala el B, y se
  comprueba que la búsqueda reindexa en vez de mezclar.
- **Degradación**: se borra el archivo del paquete y se comprueba que la
  búsqueda sigue devolviendo resultados de BM25.
- **Cobertura**: se insertan fragmentos sin paquete, se instala uno y se
  comprueba que `model status` reporta el hueco antes de reindexar.
- **Ausencia**: la batería actual completa debe pasar sin paquete instalado y
  dar exactamente los mismos resultados que hoy.

## Distribución

`tools/construir_paquete.py` genera los paquetes; hay que publicarlos en algún
lado desde donde los usuarios los bajen, con su `.sha256` al lado. Esa
decisión —dónde se hospedan— queda fuera de este diseño, pero sin ella
`mem model install` no le sirve a nadie más que a quien genere el paquete en
su propia máquina.

## Reservas sobre las mediciones

Las consultas en inglés y portugués del corpus real son traducciones que
escribí yo después de leer las originales, así que pueden ser más canónicas
que éstas. Las comparaciones válidas son entre recuperadores dentro de una
misma dirección, no entre direcciones. Las cifras de fusión usan un BM25
reimplementado en Python que rinde por debajo del de anathem en cruzado, así
que son un piso y no un techo.
