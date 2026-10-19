# Formato del export

`mem export --out carpeta/` escribe un archivo Markdown por memoria, con
frontmatter YAML, agrupados en subcarpetas por proyecto:

    carpeta/
      api-monolito/
        2026-08-11-el-rate-limit-del-api-publico-es-170-por-minuto-42.md
      personal/
        2026-09-23-prefiere-los-mensajes-de-commit-en-imperativo-91.md

El nombre de archivo es `{created}-{slug del título}-{id}.md`. El sufijo de id
está porque el slug se corta a ocho palabras y dos memorias del mismo proyecto,
misma fecha y título largo parecido producían el mismo nombre — pasó en un
corpus real.

## El export es completo

Lo que sale alcanza para reconstruir la base en otra herramienta. **Lo único
que no viaja son los vectores**, y se recalculan con `mem model install` más
`mem reindex`: son un derivado del texto, no un dato original.

**Se exportan también las memorias `forgotten`.** `forget` oculta, no borra;
dejarlas afuera convertiría cada migración en una pérdida de datos silenciosa.
Vienen con `status: forgotten` para que quien lea el export pueda filtrarlas.

## Campos

Siempre presentes:

| campo | tipo | qué es |
|---|---|---|
| `id` | entero | clave primaria en la base de origen; a lo que apunta `supersedes` |
| `name` | texto | slug del título, para herramientas que indexan por nombre |
| `description` | texto | el título, en una línea |
| `metadata.type` | texto | siempre `project`, por compatibilidad con lectores de frontmatter |
| `project` | texto | el proyecto al que pertenece; también es la subcarpeta |
| `tags` | lista de texto | tags asignados, ordenados alfabéticamente |
| `created` | fecha | cuándo se guardó |
| `updated` | fecha | última edición |
| `status` | texto | `active`, `superseded`, `draft` o `forgotten` |
| `source` | texto | `explicit` (la pidió el usuario), `suggested` o `migrated` |
| `pinned` | booleano | si sobrevive el recorte de candidatos en la búsqueda |

Presentes solo cuando tienen valor — se omite la clave en vez de emitir `null`,
porque un frontmatter sin la clave se lee igual en cualquier parser y no invita
a confundir "no tiene" con "tiene nulo":

| campo | tipo | qué es |
|---|---|---|
| `ext_id` | texto | identificador en el sistema del que se migró |
| `supersedes` | entero | el `id` de la memoria que esta reemplaza |
| `topic` | texto | clave de tema con la que se guardó; la vigente de cada clave es la única `active` entre las que la comparten en un proyecto |
| `event_start` | fecha | inicio del período al que se refiere |
| `event_end` | fecha | fin de ese período |
| `refs` | lista de `{kind, value}` | tickets, PRs y commits extraídos del cuerpo; `kind` es `ticket`, `pr` o `sha` |

El cuerpo de la memoria va después del frontmatter, tal cual, sin transformar.

## Sobre el YAML

Todos los valores de texto se emiten entre comillas dobles y con escapes. No es
cosmético: sin eso, un título con `": "` adentro hacía que el parser lo leyera
como una clave anidada y el archivo dejaba de ser YAML válido — pasó en 52 de
170 archivos de un corpus real.

## La otra salida

El export es cómodo para leer y para importar en herramientas que entienden
Markdown. Pero **la salida completa de verdad es `~/.anathem/mem.db`**: SQLite
plano, formato abierto y documentado, legible con cualquier herramienta sin
pasar por anathem. Si el objetivo es no quedar encerrado, ese archivo es la
garantía; el export es la comodidad.
