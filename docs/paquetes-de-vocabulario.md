# Paquetes de vocabulario

La búsqueda de anathem funciona sin ningún modelo: BM25 sobre FTS5, cero
descargas. Un paquete de vocabulario agrega una capa vectorial que encuentra
memorias escritas en un idioma cuando preguntás en otro.

**Los idiomas no son módulos.** El modelo es una sola tabla con un espacio
vectorial compartido, destilado sobre todos los idiomas a la vez, y esa
destilación conjunta es lo que alinea un término con su traducción. No se
puede agregar un idioma a un paquete existente. Lo que se elige es **qué
porción de la tabla se descarga**, y sumar un idioma es generar un paquete
nuevo que lo incluya.

## Por qué un paquete es tanto más chico

El modelo base tiene 500.353 entradas de vocabulario y pesa 512 MB en fp32.
Inglés y español juntos ejercitan **el 15 % de esas entradas**. Recortar el
resto y cuantizar a int8 da un paquete de 22,5 MB que rinde igual que el
modelo completo:

| | disco | mMARCO en | mMARCO es | corpus real es | corpus real en |
|---|---|---|---|---|---|
| modelo completo, int8 | 128 MB | 69 % | 48 % | 60 % | 62 % |
| paquete `en-es` | **22,5 MB** | 68 % | 48 % | 62 % | 64 % |

(recall@5; las diferencias están dentro del ruido de medición)

El recorte no pierde calidad porque el tokenizador es de tipo **Unigram con
byte fallback**. Al quitar piezas, el algoritmo re-segmenta el texto entre las
que quedan en lugar de producir desconocidos, y cualquier carácter sin pieza
propia cae en los tokens de byte, que el generador siempre conserva. Encima la
cuantización se recalibra sobre las filas que sobreviven, así que la rejilla
int8 queda más fina justo donde importa.

Nada de esto aplicaría a un transformer: sus pesos son compartidos entre todos
los tokens y no admiten recorte por palabra.

## Generar un paquete para otros idiomas

Hace falta una muestra de texto genérico en los idiomas que querés cubrir: un
archivo de texto plano por idioma, una línea por pasaje. Sirve cualquier
corpus público —Wikipedia, mMARCO, MIRACL— con decenas de miles de líneas. La
muestra solo determina qué piezas del vocabulario se conservan, así que
conviene que sea variada y **que no sea el corpus que después vas a indexar**.

    python3 tools/construir_paquete.py \
        --nombre en-es-pt \
        --muestra textos_en.txt textos_es.txt textos_pt.txt \
        --salida en-es-pt.anathem-pack

Escribe el paquete y un `.sha256` al lado. El tamaño crece mucho menos que
lineal con los idiomas, porque comparten piezas: el paso de `en-es` a
escritura latina completa (que cubre además portugués, francés, italiano y
alemán) lleva la tabla de 19,5 MB a 70,7 MB.

Después de generarlo, medilo antes de publicarlo. Un paquete con la muestra
mal elegida no falla: rinde peor y en silencio.

## Cambiar de paquete obliga a reindexar

Dos paquetes distintos producen vectores que no son comparables entre sí.
anathem guarda en la base con qué paquete se construyó el índice y **reindexa
al detectar un cambio, nunca mezcla**. Reindexar cuesta unos 0,3 segundos cada
10.000 fragmentos, así que cambiar de paquete es barato.

## Formato del archivo

Un solo archivo, todo en little endian:

| campo | tipo | notas |
|---|---|---|
| magic | 4 bytes | `ANPK` |
| versión | u32 | 1 |
| dimensiones | u32 | 256 en el modelo base |
| tokens | u32 | filas de la tabla |
| largo del nombre | u32 | |
| nombre | bytes UTF-8 | identificador, p. ej. `en-es` |
| largo del tokenizador | u64 | |
| tokenizador | bytes UTF-8 | `tokenizer.json` recortado |
| escalas | f32 × dimensiones | una por dimensión |
| mínimos | f32 × dimensiones | una por dimensión |
| tabla | u8 × tokens × dimensiones | fila mayor |

Para recuperar un vector: `valor = byte * escala[d] + mínimo[d]`. El embedding
de un texto es el promedio de las filas de sus tokens, normalizado.

El `.sha256` cubre el archivo entero. `mem model install` lo verifica y
rechaza el paquete si no coincide.
