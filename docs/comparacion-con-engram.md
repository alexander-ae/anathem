# anathem contra Engram

[Engram](https://github.com/Gentleman-Programming/engram) es lo más parecido a
anathem que existe: un binario de Go, SQLite con FTS5, sin demonio, sin Node ni
Python ni Docker, servidor MCP para cualquier agente. Mismas decisiones de
fondo, tomadas por separado.

Y en producto está más adelante: v3.4.0, releases para Linux, macOS y Windows
en las dos arquitecturas, instalación por Homebrew, TUI, API HTTP, CLI, y 23
herramientas MCP contra las 4 de anathem.

Así que la pregunta honesta es si anathem tiene razón de existir. Esto lo mide.

## El banco

Las mismas 449 memorias y las mismas 50 consultas en los dos sistemas. Las
memorias se importaron a Engram con su propio `engram save`, una por una, y se
consultó con `engram search`. Las consultas son conversacionales a propósito:
describen la situación en vez de nombrarla, que es como pregunta una persona.

| | devueltos | ms | tokens | r@1 | r@5 | r@20 | parafrásticas @20 |
|---|---|---|---|---|---|---|---|
| **anathem** | 20 | **8** | **1.303** | 28 % | **52 %** | **74 %** | **31 %** |
| Engram `--match all` (por defecto) | 0 | 427 | 28 | 4 % | 4 % | 4 % | 0 % |
| Engram `--match any` | 20 | 434 | 2.475 | 0 % | 0 % | 0 % | 0 % |
| Engram `all`, sin palabras vacías | 0 | 433 | 25 | 6 % | 6 % | 6 % | 0 % |
| Engram `any`, sin palabras vacías | 20 | 443 | 2.458 | **30 %** | 40 % | 54 % | 25 % |

## Qué dice

**Engram no está pensado para consultas conversacionales.** Con su modo por
defecto —`--match all`, que exige todas las palabras— una pregunta de diez
palabras no devuelve nada: 4 % de recall porque solo aciertan las dos
consultas más cortas. Con `--match any` devuelve veinte resultados pero
engancha las palabras vacías (`de`, `el`, `que`) y ordena por algo que en la
práctica se parece a la recencia: 0 % de recall, los veinte son las memorias
más nuevas.

**Su mejor configuración exige preprocesar la consulta.** Quitándole las
palabras vacías a mano —algo que Engram no hace— sube a 30 % en r@1, que es
**mejor que anathem** (28 %). Es un resultado real y hay que decirlo.

**Pero cae en profundidad.** A r@5 anathem saca 12 puntos de ventaja, a r@20
saca 20. Y en las consultas parafrásticas, 31 % contra 25 %.

**Y cuesta el doble de tokens.** 2.458 contra 1.303 por consulta, porque
devuelve el cuerpo completo de cada resultado en vez de un extracto.

**La latencia es la diferencia más grande, y es real.** Medí el arranque del
binario de Engram por separado: 2 ms. Así que esos 435 ms son búsqueda, no
arranque — la comparación no está sesgada por el método. anathem resuelve en
8 ms con el servidor caliente y 108 ms en frío (incluyendo cargar el paquete
de 22,5 MB). Engram es unas 54 veces más lento por búsqueda.

**Y no busca entre idiomas.** Es lo que anathem agrega con el paquete
opcional, y lo que ninguno de los trece servidores de memoria comparados
documenta.

## Cuándo elegir cuál

**Engram**, si querés algo que ya está terminado y no te importa formular con
palabras clave en vez de preguntas. Se instala con Homebrew, tiene TUI, API
HTTP, y muchas más herramientas MCP. Para un corpus en un solo idioma
consultado con sustantivos concretos, su r@1 es competitivo.

**anathem**, si preguntás en prosa, si tu corpus mezcla idiomas, o si te
importa el costo en tokens y la latencia. Las tres cosas están medidas arriba.

## Las reservas

- Las cifras de anathem acá (74 % de r@20) son más bajas que las de
  `medicion-corpus-real.md` (82 %), porque el corpus creció de 174 a 449
  memorias. Más distractores, misma pregunta.
- El quitado de palabras vacías lo hice yo con una lista propia. Engram
  podría rendir distinto con otro preprocesamiento.
- Importé las memorias con `engram save` título por título; si su ingesta
  normal —por MCP, desde un agente— indexa distinto, los números podrían
  cambiar.
- n=50, y 16 parafrásticas. Las diferencias de pocos puntos son ruido; las de
  veinte, no.
