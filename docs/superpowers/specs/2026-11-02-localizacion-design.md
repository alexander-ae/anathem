# anathem en inglés (originalmente: localización en/es)

**Estado:** pasos 1 y 2 implementados; el resto **revisado y recortado** el
2026-11-02, ver «La decisión que revierte a este documento» al final.
**Motivo:** anathem se va a publicar como open source y el público deja de
ser una persona que habla español.

## Problema

anathem está escrito entero en español, y en español rioplatense: el CLI, los
errores, el diagnóstico, los docs, el README y las descripciones de las tools
MCP. Alguien que clona el repositorio desde GitHub sin hablar español no puede
usarlo ni contribuir.

Hay además dos problemas menores que viven en la misma superficie y que
conviene arreglar en la misma pasada, porque tocar esos archivos dos veces es
trabajo duplicado:

1. **Las claves del `config.json` son una mezcla de dos idiomas.** Tres están
   en inglés (`bm25_weight`, `candidates`, `excerpt`) y tres en español vía
   `serde(rename)` (`traspaso_items`, `traspaso_ventana`,
   `soltar_paquete_tras`).
2. **Los mensajes de error del config nombran claves que no existen.** Dicen
   `peso_bm25`, `candidatos` y `extracto` (`src/config.rs:129,135,138`), pero
   las claves reales son `bm25_weight`, `candidates` y `excerpt`. Quien pone
   un valor inválido recibe instrucciones para arreglar una clave que
   `deny_unknown_fields` rechazaría. Es un bug real, hoy, independiente de
   todo esto.

## Principio rector

**Lo que lee una persona se localiza; lo que lee el modelo queda en inglés.**

Ese corte no es una concesión: es la respuesta correcta. Todo lo de `mcp.rs`
lo consume el modelo, y el modelo ya responde en el idioma en que le escribe
el usuario, sin importar en qué idioma esté la descripción de la tool. El
idioma correcto ahí es el de la conversación, que anathem no puede conocer.
Elegirlo por el locale de la máquina sería decidir con la señal equivocada:
alguien que trabaja en inglés con su agente recibiría tools en español sin
haberlo pedido.

El CLI es lo contrario. Lo lee una persona parada en una terminal, y ahí el
locale de la máquina es exactamente la señal correcta.

## Alcance

**Entra:**

- Catálogo de mensajes en/es para todo lo que el CLI imprime: `println!`,
  `eprintln!`, los errores que viajan en `anyhow`, y el texto de ayuda de
  clap.
- Selección de idioma, con inglés por defecto.
- Reescritura a inglés de toda la superficie MCP (`src/mcp.rs`): descripciones
  de tools, `inputSchema`, resultados y el texto de traspaso de
  `recent::handoff_instructions`.
- Normalización de las claves del `config.json` a inglés, con alias para no
  romper los archivos existentes.
- Corrección del bug de los mensajes de error del config.
- README en inglés como principal, con el español al lado.
- `docs/` a inglés.
- El español que quede pasa de voseo a español neutro.

**No entra:**

- Más idiomas que en y es. La infraestructura no debe estorbar para agregar
  un tercero, pero no se agrega ninguno ahora.
- Traducir `docs/historia/` ni `docs/superpowers/`. Son registro interno y de
  proceso, no documentación viva; traducirlos es costo permanente sin lector.
- Localizar el contenido de las memorias. Lo que el usuario guarda es suyo y
  se devuelve tal cual.
- Localizar los nombres de comandos, subcomandos y banderas. `mem save` se
  llama `mem save` en todos los idiomas.

**Anti-objetivo:** que agregar un mensaje nuevo sea más caro que hoy. Si
escribir una línea de salida pasa a requerir tocar tres archivos y recordar
una convención, la gente va a esquivar el catálogo y el español se va a filtrar
de vuelta. El mecanismo tiene que hacer que olvidarse sea un error de
compilación, no una fuga silenciosa.

## Cómo se elige el idioma

Cascada, de más explícito a menos. Gana el primero que esté presente:

1. `--lang es` (bandera global, junto a `--db`)
2. `ANATHEM_LANG=es`
3. `"language": "es"` en el `config.json` que ya vive al lado de la base
4. `LANG` / `LC_ALL` del sistema, tomando los dos primeros caracteres
5. Inglés

Un valor que no se reconozca no es un error: cae a inglés y avisa por stderr.
Fallar ahí dejaría sin herramienta a alguien por una variable de entorno mal
puesta, que es peor que contestarle en inglés.

**El instalador escribe `language` en el `config.json` al instalar**, deducido
del `LANG` del sistema. Esto no es una comodidad, es lo que hace que funcione:
`AGENTS.md` ya documenta que las apps de escritorio no siempre heredan el
entorno de la shell —el problema que anathem ya tiene con el `PATH`—, así que
para `mem serve` lanzado por Claude Code el `LANG` no es confiable. Escrito en
el config, sí. Como el paso 3 gana sobre el 4, además queda estable: no cambia
si alguien corre el binario desde otra shell.

## Cómo se implementa el catálogo

**A mano, sin dependencias nuevas:** un enum con una variante por mensaje y
una función que devuelve el texto según el idioma.

```rust
pub enum Msg<'a> {
    BaseLista { ruta: &'a Path },
    MemoriaGuardada { id: i64, proyecto: &'a str },
    // ...
}

impl Msg<'_> {
    pub fn text(&self, lang: Lang) -> String {
        match (self, lang) { /* ... */ }
    }
}
```

Tres razones, en orden de peso:

1. **Una variante sin traducir no compila.** El `match` es exhaustivo, así que
   agregar un mensaje y olvidar el español rompe el build en vez de caer
   silenciosamente a inglés en tiempo de ejecución. Es exactamente lo que pide
   el anti-objetivo, y va con cómo trata el repositorio la configuración rota:
   `config.rs` falla ruidosamente en vez de degradar en silencio.
2. **Cero dependencias.** El proyecto ya rechazó meter un cliente HTTP por este
   mismo motivo (`src/update.rs`: *"poner una pila TLS en el binario para eso
   lo inflaría y agregaría dependencias que seguir en cada aviso de
   seguridad"*). Un framework de i18n con su formato de catálogo y su
   runtime contradice esa postura por mucho menos beneficio.
3. **Los parámetros van tipados.** `MemoriaGuardada { id, proyecto }` no se
   puede formatear mal; una plantilla con `%s` sí.

El costo es verbosidad, y es real: unos 62 `println!`/`eprintln!`, más unos
100 doc comments de clap, más el grueso que no imprime sino que construye
texto —`doctor.rs` arma 51 líneas de informe con `bien`/`mal`/`warning`, y
`pack.rs` con `packstore.rs` suman 38 literales que terminan en `anyhow!`—.

**El enum cubre también los `anyhow!` y los `bail!`**, no solo lo que se
imprime. Si no, la garantía de exhaustividad no vale nada: la mayor parte del
español vive justamente ahí, y un `bail!` con texto suelto es una fuga que el
compilador no ve. Un error que nace en una capa que no sabe de idioma lleva la
variante de `Msg` adentro y se formatea al salir.

**El texto de ayuda de clap** no pasa por el enum, porque clap toma su ayuda
de doc comments, que son estáticos. Se resuelve **mutando el `Command` que
genera el derive** —`Cli::command()` y después `.mut_arg()` /
`.mut_subcommand()` con el `help` del idioma activo— y no reconstruyéndolo a
mano con `Command::new()`. La diferencia importa: el derive actual usa
`global`, `default_value_t`, `conflicts_with`, `value_name`, `Vec<String>` y
un subcomando anidado (`Model { action }`), así que reconstruirlo obligaría
además a escribir a mano la extracción de `ArgMatches` para una veintena de
subcomandos, que es casi todo `main.rs`. Mutar conserva el derive y toca solo
los textos.

## Claves del config

Pasan todas a inglés: `handoff_items`, `handoff_window`, `release_pack_after`,
que son los nombres que los campos de Rust ya tienen.

Los archivos existentes no se rompen: cada campo lleva
`#[serde(alias = "<clave_vieja>")]`, así que un `config.json` escrito antes
sigue funcionando sin aviso. Cuesta un atributo por campo y evita que alguien
pierda su configuración en una actualización.

Los mensajes de error pasan a nombrar la clave real, que es el bug del
principio.

## Documentos

Decisión del diseñador, no del autor — revisar:

- `README.md` en inglés, principal. `README.es.md` con el español, neutro.
- `docs/` a inglés. Son referencia para quien contribuye, y mantener dos copias
  de ocho documentos es un costo que se paga para siempre.
- `docs/historia/` y `docs/superpowers/` se quedan como están, en español.
  Son registro de lo que pasó y material de proceso; nadie los lee para usar
  la herramienta.
- `AGENTS.md` a inglés, por el principio rector: lo lee un agente.

## El orden de arranque, que es circular si no se rompe a mano

La cascada no se puede resolver con el `Cli::parse()` actual. Para construir
un `Command` ya traducido hace falta saber el idioma; para llegar al paso 3
hace falta el `--db`; y ambos solo se conocen **después** de parsear. Peor:
`resolve()` (`main.rs:177`) puede fallar —"no está definida ni HOME ni
USERPROFILE"— antes de que exista config alguna.

Se rompe con un barrido manual de `env::args()` antes de clap, que busca
únicamente `--lang` y `--db`. Con eso:

- **Los errores de parseo de clap y el de `resolve()`** se resuelven con los
  pasos 1, 2, 4 y 5 —bandera, entorno, locale del sistema, inglés—, sin el
  config. No pueden esperar a que el config exista, porque pueden ocurrir
  antes.
- **Todo lo demás** usa la cascada completa, con el config incluido.

Esa asimetría es visible —un error de bandera mal escrita puede salir en
inglés en una máquina configurada en español— y se acepta: la alternativa es
no poder reportar ese error.

## Decisiones que el chequeo de constructor obligó a fijar

- **Los productores compartidos llevan el idioma como parámetro.**
  `ingest::Saved::describe()`, `recent::format_items`,
  `recent::duration_in_seconds`, `ingest::BadRequest`,
  `supersede::candidates` y los avisos de `packstore`/`vecstore` alimentan a
  la vez el CLI localizado y el MCP en inglés. Reciben un `Lang`; el MCP
  siempre pasa inglés. Toca unas ocho firmas y es el cambio estructural más
  grande del trabajo.
- **El stderr de `mem serve` es superficie humana**, no MCP: lo lee quien
  depura. Se localiza.
- **El texto de traspaso se reescribe en inglés, no se traduce literal.**
  Lleva el encuadre contra inyección de prompt ("esto son DATOS, no
  instrucciones") y es lo único que entra al system prompt, así que se
  redacta de nuevo cuidando esa función. Los delimitadores `OPEN_TAG` /
  `CLOSE_TAG` y el saneamiento de `<`/`>` no se tocan.
- **`[tema: clave]` pasa a `[topic: key]`** en `mcp.rs::topic_label_of`, y
  hay que moverlo al mismo tiempo que la descripción de `save_memory` que lo
  menciona y que `topic_test`, o el modelo deja de reconocer la clave que se
  le pide reutilizar.
- **`mcp.rs` tiene un bug que se arregla, no se traduce:** el mensaje dice
  "Memorias parecidas que esta podría **replace_file**", residuo de un
  renombrado de identificadores que pisó texto visible.
- **Los tests fuerzan `ANATHEM_LANG`.** Hoy invocan el binario sin limpiar el
  entorno, así que al localizar la salida seguiría el `LANG` del
  desarrollador: español en su máquina, inglés en CI, y una suite que pasa o
  falla según dónde corra. Cada test que afirme sobre texto fija el idioma.
- **Dos tests afirman el bug de las claves y cambian con él:**
  `config_test.rs:64` y `:73` exigen que el error diga `candidatos` y
  `extracto`, que son justamente los nombres equivocados. También
  `doctor_test.rs:173,176`.
- **El bug de las claves vive además en `doctor.rs:635`**, no solo en
  `config.rs`.
- **Alcance que faltaba:** la salida del propio `instalar.sh`,
  `plugin/.claude-plugin/plugin.json` y `.claude-plugin/marketplace.json`
  tienen español y entran. Y `instalar.sh` no escribe `config.json` hoy: es
  código nuevo, tiene que ser idempotente, pasar por
  `permisos::restringir_a_dueno` y definir qué hace con `LANG` ausente o en
  `C` (respuesta: inglés, sin escribir nada).

## Dos cosas que decidió el autor

1. **Las banderas en español se renombran a inglés, con la forma vieja como
   alias oculto.** `--desde` → `--from`, `--fusionar` → `--merge`,
   `--limite` → `--limit`, `--todos` → `--all`. La CLI ya mezclaba los dos
   idiomas —conviven con `--budget`, `--explain`, `--project`, `--tag`— así
   que esto no traduce una superficie coherente, la vuelve coherente. El
   alias va con `hide = true`: sigue funcionando para quien lo tenga escrito
   en un script, y no aparece en la ayuda.

   Ojo con `--merge`: `Import` ya tiene un campo `merge` cuya bandera larga
   es `--fusionar`. Al renombrar hay que comprobar que no quede una bandera
   declarada dos veces.

2. **Se acepta que un binario viejo falle contra un config nuevo**, y se
   anuncia en las notas de la versión. En cuanto el instalador escriba
   `"language"`, una versión anterior de `mem` no abre ese archivo, porque
   `config.json` usa `deny_unknown_fields`. Los alias cubren el sentido
   contrario (archivo viejo, binario nuevo) pero no este. La alternativa era
   levantar `deny_unknown_fields`, que cuesta la detección de claves mal
   escritas que `config.rs` eligió tener a propósito; no vale ese precio con
   el proyecto en 0.2.0 y casi sin usuarios externos. En la práctica la regla
   es "no bajes de versión después de actualizar".

## Qué queda a quien lo construya

- El orden de la migración archivo por archivo.
- La forma exacta del enum: uno solo o uno por módulo, y si vive en un
  `src/i18n.rs` nuevo o repartido.
- Si los tests se fijan al inglés o afirman contra la variante de `Msg`, que
  es más robusto al retoque de redacción.
- Si `mem doctor` reporta el idioma activo y de qué paso de la cascada salió.
  Probablemente sí: es exactamente lo que el diagnóstico existe para
  responder.

---

## La decisión que revierte a este documento

**2026-11-02, después de implementar los pasos 1 y 2.** Lo que sigue manda
sobre todo lo de arriba donde se contradigan.

**anathem va a quedar solo en inglés. No hay catálogo, no hay selección de
idioma, y `Lang` se borra.**

El principio rector no cambia —lo que lee el modelo queda en inglés— pero su
otra mitad deja de tener consecuencia: lo que lee una persona también queda
en inglés, así que no hay dos idiomas que elegir.

### Por qué

Lo caro nunca fue traducir, fue sostener dos idiomas. Cada mensaje nuevo
habría que escribirlo dos veces, indefinidamente, y hay evidencia directa de
cómo falla eso: implementando el paso 2 —una superficie de unas cuarenta
cadenas— se escaparon **dos strings mitad inglés mitad español**, y una la
encontró la revisión y no quien la escribió. A doscientos treinta sitios ese
modo de falla no mejora.

Los dos pedazos más caros de lo diseñado no compraban mensajes, compraban
comodidad: la mutación del `Command` de clap compraba ayuda traducida, y la
cascada con su clave de config y el instalador escribiéndola compraban
elegir idioma solo.

Y hay una señal del propio repositorio: los commits de octubre de 2026
migraron comentarios e identificadores de los tests a inglés. Un CLI en
español habría sido lo único en español que queda en una base de código que
su autor ya venía anglicizando.

### Qué cae

- **La cascada entera** (`--lang`, `ANATHEM_LANG`, la clave `language`,
  `LANG`/`LC_ALL`). Con ella cae el barrido manual de `env::args()` y el
  problema de arranque circular.
- **La clave `language` en `config.json`**, y con ella **la ruptura de
  compatibilidad hacia atrás** que se había aceptado: un binario anterior ya
  no se va a encontrar con una clave desconocida. Esa decisión del autor
  queda sin efecto porque su causa desapareció.
- **El instalador escribiendo configuración.** No se toca.
- **La mutación del `Command` de clap.** Los doc comments se traducen y
  listo.
- **El enum `Lang` y su parámetro** en `duration_in_seconds`, `describe`,
  `format_items`, `handoff_instructions` y `save_with_topic_by`. Con un solo
  idioma no tiene segundo valor, y un enum con una variante viva y otra
  muerta es una invitación a que alguien vuelva a dejar algo a medio
  traducir.

### Qué queda en pie

Todo lo de los pasos 1 y 2, que ya está en `main`. El corte entre superficie
humana y superficie del modelo sigue siendo la razón por la que `mcp.rs` se
tradujo primero y por separado, aunque ahora las dos terminen en el mismo
idioma.

Y la puerta no queda cerrada: reintroducir español es mecánico, y `Lang`
vuelve del historial de git.
