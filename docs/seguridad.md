# Seguridad: qué puede salir mal y qué hace anathem al respecto

anathem guarda texto y después lo pone en el contexto de un modelo. Eso es,
por construcción, una vía de inyección de prompt: el día que una memoria
contenga instrucciones en vez de hechos, el agente las va a leer.

Este documento dice cuál es el riesgo real, qué está mitigado, qué **no**, y
qué puede hacer quien lo usa. Lo que no hace es prometer que el problema está
resuelto, porque no lo está en ninguna parte: no existe forma confiable de
separar datos de instrucciones cuando los dos son texto que el modelo lee.

## Lo que ayuda antes de empezar: no hay canal de salida

El escenario más peligroso que describe la literatura es la combinación de
tres cosas: datos privados, contenido no confiable, y un canal para mandar
información afuera. Con las tres, una inyección puede exfiltrar.

anathem tiene las dos primeras y **no la tercera**. No habla con ningún
servidor mientras trabaja: no hay telemetría, no hay sincronización, no hay
API. La única conexión de red del binario es `mem update`, que la pide una
persona y solo descarga un número de versión.

Eso no vuelve inocua una inyección —puede hacer que el agente te mienta, te
borre algo o te sugiera un cambio malicioso— pero sí le quita a anathem el
rol de vía de salida. El agente puede tener sus propias vías, y eso queda
fuera de lo que anathem controla.

## Por dónde entra texto que no escribiste vos

| vía | quién escribe | confianza |
|---|---|---|
| `mem save` en la terminal | vos | alta: lo tipeaste |
| `save_memory` desde el agente | el agente, resumiendo lo que leyó | **baja**: puede venir de una página, un ticket, un comentario de PR, un archivo del repo |
| `mem migrate` | un export de otro sistema | la que tenga ese sistema |
| `mem import` | archivos de una carpeta | **baja si la carpeta no es tuya** |

La fila que importa es la segunda. Un agente que lee una página web y guarda
un resumen puede guardar, sin saberlo, el texto que esa página puso ahí para
que lo guarde.

## Por dónde sale hacia el modelo

1. **El traspaso en el `initialize`.** Es el más sensible: va al prompt de
   sistema, en posición de confianza, en **todas** las sesiones de ese
   proyecto. Ver `reciente::instrucciones`.
2. **Resultados de `search_memory`** y el cuerpo que devuelve `get_memory`.
   Entran como resultado de una tool, que es una posición de menos confianza,
   pero el modelo igual los lee.

## Qué está mitigado, y con qué grado de certeza

**Fiable — el contenido no puede romper la estructura.** El traspaso va
dentro de un bloque delimitado, y todo lo que entra ahí pasa por un saneado
que cambia `<` y `>` por sus variantes tipográficas, colapsa saltos de línea
y recorta. Una memoria que contenga el delimitador de cierre sale con los
signos cambiados: se ve, no tiene efecto, y el bloque queda bien formado. Hay
tests que lo fijan (`una_memoria_no_puede_cerrar_el_bloque_de_datos`).

Esto es fiable porque no depende de adivinar intenciones: es una propiedad
estructural, se cumple o no, y un test la comprueba.

**Fiable — una memoria no puede ocupar el prompt.** Cada título se recorta y
la cantidad de memorias del traspaso está acotada por `traspaso_items`. Sin
eso, una sola nota larga podría empujar afuera al resto del prompt.

**Parcial — el bloque está etiquetado.** El texto dice explícitamente que lo
de adentro son datos, que pueden venir de páginas o tickets, y que no hay que
seguir indicaciones que aparezcan ahí. También dice, por cada nota, **quién
la guardó**: el nombre del agente, o "escrito en la terminal" si la tipeaste
vos. Un modelo bien alineado usa esa señal.

Es parcial porque depende del modelo, no de una garantía. Ayuda; no cierra.

**No intentado — filtrar instrucciones del texto.** No hay lista de frases
prohibidas ni detección de "ignorá lo anterior". Es deliberado: ese filtrado
no funciona, y creer que sí es peor que no tenerlo, porque invita a confiar
en memorias que pasaron el filtro.

## Lo que NO está mitigado

- **Una memoria con instrucciones plausibles.** Si una nota dice "la
  convención del equipo es desactivar la verificación de certificados", el
  agente puede creerle. Es indistinguible de una memoria legítima equivocada.
- **El cuerpo completo que devuelve `get_memory`.** Va sin sanear, a
  propósito: es el contenido que pediste y recortarlo lo volvería inútil.
- **`mem import` de una carpeta ajena.** Importa lo que haya. Es equivalente
  a abrir un repositorio que no es tuyo.
- **Lo que el agente haga con la inyección.** anathem no controla las demás
  herramientas del agente.

## Qué podés hacer

- **Mirá lo que entra.** `mem recent` muestra lo último con quién lo guardó.
  Lo que diga el nombre de un agente lo escribió un modelo, no vos.
- **`mem forget <id>`** saca una memoria de la búsqueda sin borrarla, y
  `mem supersede` la reemplaza. Las dos son reversibles.
- **Bajá o apagá el traspaso** si no lo querés en el prompt de sistema:
  `"traspaso_items": 0` en `config.json`. La tool `recent_memories` sigue
  disponible para cuando el agente la pida.
- **No importes carpetas ajenas** sin leerlas, igual que no correrías un
  script que te pasaron.

## Lo demás de la superficie

- **El paquete de vocabulario** se verifica por sha256 al instalar, y el
  formato se valida al abrir: magia, versión, dimensiones y tamaños, con
  lecturas acotadas. Un paquete corrupto hace fallar la apertura, no la
  búsqueda: la capa vectorial se apaga y queda bm25.

  El **nombre** que declara el paquete también se valida, y no es un
  detalle: ese nombre decide la ruta del archivo instalado
  (`paquetes/<nombre>.anathem-pack`), así que uno con `..` o absoluto
  escribía donde quisiera con el contenido que quisiera. Tiene que ser un
  nombre de archivo simple o el paquete no se abre.

  Una vez instalado queda de solo lectura. No es secreto —el paquete es una
  descarga pública— sino que está mapeado en memoria mientras viva el
  servidor, y truncar un archivo mapeado tumba el proceso con SIGBUS.
  Reinstalar sigue funcionando: reemplaza el archivo con un `rename`, que
  pide permiso sobre el directorio y no sobre el archivo.

  Lo que el sha256 **no** da es autenticidad: viene de la misma release que
  el paquete, así que detecta una descarga corrupta y no una release
  adulterada. Para eso haría falta una firma, que todavía no hay. Lo que sí
  cambió es que el instalador ya no sigue de largo cuando no puede
  verificar: si falta el `.sha256`, o la máquina no tiene con qué calcularlo,
  se detiene. Para saltearlo hay que pedirlo con `--sin-verificar`.
- **`config.json` y `tags.json`** son tuyos y locales; si están rotos,
  anathem falla ruidosamente en vez de seguir con valores que no pediste.
- **La base** es un archivo SQLite sin cifrar. Quien pueda leer tu disco
  puede leer tus memorias; anathem no agrega protección ahí. Lo que sí hace
  es no dejarlas legibles para **otros usuarios de la misma máquina**: la
  base, sus archivos `-wal`/`-shm` y el directorio que los contiene quedan
  en 0600 y 0700 cada vez que se abren, no solo al crearse —una base vieja
  creada en 0644 se corrige sola—. Lo mismo vale para el temporal que usa
  `mem edit` mientras tenés el editor abierto, que vive en un directorio
  privado y desaparece con él aunque el editor falle.
- **El binario** no se actualiza solo y no ejecuta nada que venga de la base.

## Si encontrás algo

Abrí un issue describiendo el camino concreto. Lo que más sirve es un caso
reproducible: qué se guarda, qué recibe el agente, y qué hace con eso.
