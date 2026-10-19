# Las cifras de mMARCO no se trasladan a memorias

`medicion-multiidioma.md` midió sobre mMARCO y cerró con una advertencia: los
pasajes web tienen forma léxica, las memorias no. Esta medición comprueba esa
advertencia sobre un corpus real de memorias — 170 memorias activas, 1.024
fragmentos, 50 consultas de evaluación — y la advertencia era correcta. La
dirección se sostiene; las magnitudes no, y el error va a favor.

Las 50 consultas originales están en el idioma del corpus. Se tradujeron a
inglés y portugués conservando los identificadores tal como aparecerían si el
usuario preguntara en ese idioma (números de ticket, nombres de usuario,
banderas de migración, nombres de herramientas).

## Resultados

| dirección | BM25 r@5 | BM25 r@50 | estático r@5 | estático r@50 |
|---|---|---|---|---|
| mismo idioma | 66 % | 88 % | 60 % | 88 % |
| inglés → corpus | 34 % | **80 %** | 62 % | **94 %** |
| portugués → corpus | 56 % | **80 %** | 50 % | 86 % |

Comparado con mMARCO, donde BM25 cruzado daba 21 % en r@5 y **33 %** en r@50.

## Qué cambia

En mMARCO concluí que ningún reranker puede ayudar en el caso cruzado, porque
con recall@50 del 33 % el documento no llega al conjunto de candidatos. **Sobre
memorias reales esa conclusión no se sostiene**: el recall@50 cruzado es 80 %,
así que el documento correcto casi siempre está entre los candidatos y lo que
falla es el orden. Eso es precisamente el problema que un reranker sí resuelve.

La causa es la forma del texto. Una memoria técnica está saturada de tokens que
sobreviven a la traducción — identificadores de tickets, nombres de usuario,
nombres de cabeceras HTTP, hashes de commit, banderas de migración, y el
vocabulario técnico en inglés que se usa sin traducir. BM25 no entiende la
consulta, pero engancha bastantes de esas anclas como para meter el documento
entre los cincuenta primeros. mMARCO no tiene anclas de este tipo.

El portugués se comporta distinto del inglés en cada componente: BM25 lo maneja
mejor (56 % contra 34 %) por cercanía léxica con el idioma del corpus, y el
modelo estático lo maneja peor (50 % contra 62 %). Ninguno de los dos domina en
los dos idiomas.

## Fusión

| dirección | RRF 1,0 / 1,0 | RRF 0,25 / 1,0 |
|---|---|---|
| mismo idioma | **68 %** | 64 % |
| inglés → corpus | 58 % | **74 %** |
| portugués → corpus | **62 %** | 60 % |

Se repite lo de mMARCO: peso bajo a BM25 ayuda en cruzado y estorba en
monolingüe, y no hay un peso que gane en ambas direcciones. La fusión en
inglés llega a 74 %, por encima de los dos componentes por separado y por
encima del monolingüe.

## Dos reservas sobre estas cifras

**Las traducciones las escribí yo, después de haber leído las consultas
originales.** No puedo descartar haber producido consultas más canónicas que
las originales, que están redactadas de modo informal. Que el caso inglés
supere al monolingüe es sospechoso y probablemente sea este artefacto y no una
propiedad del sistema. Las comparaciones válidas son entre recuperadores dentro
de una misma fila, no entre filas.

**La fila de fusión usa un BM25 reimplementado en Python**, porque el evaluador
solo emite métricas y no rankings completos. Ese BM25 rinde por debajo del de
anathem en cruzado (22 % contra 34 % en inglés, 48 % contra 56 % en portugués),
así que las cifras de fusión son un piso, no un techo.

## Aislamiento

Se trabajó sobre una copia de la base, fuera del repositorio. La base personal
quedó intacta: md5 `fe6c78006c8cf99f51bd9ed9e7da5dbc`, 171 memorias.

## Corrección: el peso por defecto es 1,0, no 0,25

Medido con la implementación terminada sobre el corpus real, con el paquete
`en-es` instalado y el evaluador propio de anathem —no con el BM25
reimplementado en Python de las secciones anteriores:

| | mismo idioma r@5 | inglés → corpus r@5 | portugués → corpus r@5 |
|---|---|---|---|
| solo bm25 | **66 %** | 34 % | 56 % |
| híbrido, peso 0,25 | 56 % | **62 %** | 48 % |
| híbrido, peso 1,0 | **66 %** | 58 % | 50 % |

Con peso 0,25 la búsqueda del mismo idioma **pierde 10 puntos** para ganar 4
entre idiomas. Con peso 1,0 empata con bm25 puro en el mismo idioma, gana 24
puntos en inglés y sube el recall@50 de 88 % a 92 %.

La recomendación anterior de 0,25 salió de fusionar con un BM25 reimplementado
en Python, que rinde por debajo del de anathem (22 % contra 34 % en cruzado).
Contra el BM25 real, que es más fuerte, bajarle el peso cuesta más de lo que
ese banco predecía. **El valor por defecto es 1,0**; quien busque casi siempre
en un idioma distinto del que escribe puede bajarlo en `config.json`.

La lección metodológica: un componente sustituido por una aproximación más
débil no solo baja el resultado de ese componente, sino que desplaza el óptimo
de todo lo que se calibre contra él.
