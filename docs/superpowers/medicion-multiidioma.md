# Cuánto cuesta buscar entre idiomas

Medición sobre mMARCO, corpus de 10.000 pasajes en inglés, 182 consultas
disponibles en inglés y español con los mismos identificadores. La columna
`es→en` es el caso que importa: una memoria escrita en un idioma, buscada en
otro. Datos y scripts en `~/anathem-bench/exp/`, fuera del repositorio.

## Los tres recuperadores

| recuperador | disco | en→en r@5 | es→en r@5 | es→en r@50 | indexar 10k |
|---|---|---|---|---|---|
| BM25 (lo que anathem hace hoy) | 0 MB | 79 % | 21 % | 33 % | 0,1 s |
| potion-multilingual-128M, int8 | 128 MB | 69 % | 48 % | 76 % | 0,3 s |
| MiniLM-L12-v2 multilingüe | 471 MB | 92 % | 69 % | 87 % | 12 s |

BM25 no entiende: sus aciertos cruzados son nombres propios y préstamos
(`hematuria`, `alexa`, `tortilla`), nunca comprensión. Un reranker no lo
arregla, porque con recall@50 del 33 % el documento correcto no llega al
conjunto de candidatos en dos de cada tres consultas.

## Cuánto se puede achicar el modelo estático

Es una tabla de consulta, no una red: se comprime de formas que un transformer
no admite.

| variante | disco | en→en r@5 | es→en r@5 | es→en r@50 |
|---|---|---|---|---|
| fp32 | 512 MB | 69 % | 47 % | 76 % |
| fp16 | 256 MB | 69 % | 47 % | 76 % |
| int8 | 128 MB | 69 % | 48 % | 76 % |
| int8, solo vocabulario latino | **71 MB** | 68 % | **47 %** | **76 %** |
| int8 + 128 dimensiones (PCA) | 64 MB | 61 % | 41 % | 69 % |
| int8 + 128 dim + latino | 35 MB | 62 % | 41 % | 69 % |
| int8 + 64 dim + latino | 18 MB | 52 % | 32 % | 56 % |

El vocabulario tiene 500.353 entradas y solo 276.360 son de escritura latina;
podar el resto no afecta a EN/ES/PT. **71 MB sin pérdida medible** es el punto
de corte: bajar de ahí cuesta calidad de forma inmediata y proporcional.

## Fusión con BM25

| fusión | en→en r@5 | en→en r@50 | es→en r@5 | es→en r@50 |
|---|---|---|---|---|
| solo BM25 | 77 % | 87 % | 19 % | 31 % |
| solo estático | 69 % | 86 % | 48 % | 76 % |
| RRF 1,0 / 1,0 | 77 % | 93 % | 43 % | 74 % |
| RRF 0,25 / 1,0 | 74 % | 88 % | **52 %** | 78 % |

(BM25 reimplementado en Python para poder fusionar; da 77/19 contra el 79/21
de anathem, así que el banco es fiel.)

**RRF con pesos iguales empeora el caso cruzado** (48 % → 43 %): BM25 aporta
casi solo ruido cuando la consulta no es del idioma del índice, y con peso
igual ese ruido desplaza aciertos buenos del vector. Con peso 0,25 la fusión
supera a los dos componentes por separado en cruzado, a cambio de 3 puntos en
monolingüe. No hay un peso que domine en ambas direcciones.

### Hipótesis descartada

Probé decidir el peso por consulta según la fuerza del mejor resultado de BM25,
suponiendo que una consulta en otro idioma produciría puntajes bajos. No
discrimina: la mediana es 21,4 en monolingüe y 14,1 en cruzado, con
distribuciones solapadas. El esquema adaptativo quedó igual que el de peso fijo
1,0. Si se quiere peso por consulta hace falta otra señal — detección de idioma
explícita, por ejemplo.

## Límite de esta medición

mMARCO son pasajes web, de forma léxica: consultas y documentos comparten
vocabulario. Las memorias personales son más cortas, más parafrásticas y están
llenas de identificadores. La dirección de estos resultados debería sostenerse;
las magnitudes no se trasladan. Sigue pendiente un conjunto de evaluación con
forma de memoria personal.

## Nota de aislamiento

Todo corrió en `~/anathem-bench/`. `~/.anathem/mem.db` quedó intacta:
md5 `fe6c78006c8cf99f51bd9ed9e7da5dbc`, 171 memorias, idéntica a la huella
tomada antes de empezar.

## Sumar idiomas después: paquetes de vocabulario

Los idiomas no son módulos. El modelo es una sola tabla con un espacio
vectorial compartido, destilado sobre todos los idiomas a la vez, y esa
destilación conjunta es justamente lo que alinea un término con su traducción.
No se puede agregar un idioma después sin rehacer el modelo.

Lo que sí se elige es **qué porción de la tabla se descarga**. El vocabulario
activo se midió tokenizando 90.000 pasajes genéricos en inglés y español, sin
solape con nada de lo evaluado.

| paquete | disco | mMARCO en | mMARCO es | personal es | personal en |
|---|---|---|---|---|---|
| completo, ~100 idiomas | 128 MB | 69 % | 48 % | 60 % | 62 % |
| escritura latina | 71 MB | 68 % | 47 % | 60 % | 62 % |
| inglés + español | **19,5 MB** | 68 % | 48 % | 62 % | 64 % |

Solo el 15 % del vocabulario (76.363 de 500.353 entradas) se usa en inglés y
español. De ahí que el paquete quepa en 19,5 MB de tabla, 22,5 MB con el
tokenizador — **sin pérdida medible**.

Cambiar de paquete obliga a reindexar, porque los vectores de dos paquetes
distintos no son comparables para documentos que usan tokens podados. **El
índice debe registrar con qué paquete se construyó y reindexar al detectar un
cambio, nunca mezclar.** Reindexar cuesta 0,3 s cada 10.000 fragmentos, así que
el cambio de paquete es una operación barata — propiedad del modelo estático,
que un transformer no tiene: sus pesos son compartidos y no admiten poda por
token.

Una primera medición, que descartaba tokens poniéndolos en cero, daba 2 a 4
puntos de pérdida. Era un artefacto del método: el generador real recorta el
tokenizador, y como el modelo es Unigram con byte fallback el texto se
re-segmenta entre las piezas que quedan en vez de producir desconocidos.
Además la cuantización se recalibra sobre las filas que sobreviven. Con eso la
pérdida desaparece. El procedimiento está en `docs/paquetes-de-vocabulario.md`.
