//! Política de identidad del paquete activo.
//!
//! Todo lo que decide *qué* paquete usa una base —dónde vive su archivo, si
//! el que está en disco es el mismo con el que se indexó, y qué hacer cuando
//! no lo es— vive acá y solo acá. Antes estaba repartido entre `main.rs` y
//! `mcp.rs`: la ruta `dir/paquetes/{nombre}.anathem-pack` reconstruida a mano
//! en cinco lugares, el bloque de sincronización copiado tres veces, y la
//! política entera escondida dentro de una función que se llamaba
//! `paquete_activo` pero escribía en la base y podía abortar el comando.
//!
//! Las dos reglas que ordenan este módulo:
//!
//! 1. **Sin paquete instalado no pasa absolutamente nada.** Ninguna función
//!    de acá cuesta más que una consulta a `meta` cuando no hay paquete.
//! 2. **Degradar a bm25 es aceptable; mezclar vectores de dos paquetes no lo
//!    es nunca.** Un índice mezclado devuelve resultados plausibles y
//!    equivocados, el peor desenlace para una herramienta de memoria. Ante
//!    cualquier duda —un rename que falla, una transacción que no cierra— se
//!    degrada a bm25, que es un resultado peor pero honesto.

use crate::pack::{self, Pack};
use crate::vecstore;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Extensión de los archivos de paquete. La convención ruta↔nombre —el
/// paquete llamado `N` vive en `paquetes/N.anathem-pack`— es lo que permite
/// encontrar el archivo sabiendo solo lo que dice `meta`.
pub const EXTENSION: &str = "anathem-pack";

/// Cuántos fragmentos mira `instalar` para decidir si el paquete le sirve a
/// esta memoria. Doscientos alcanzan: si ninguno de doscientos fragmentos
/// tiene una sola palabra que el paquete reconozca, no la va a tener el
/// doscientos uno.
const MUESTRA: usize = 200;

/// Cómo identifica la base al paquete con el que indexó: nombre, sha256 y
/// dimensiones. Vive en `vecstore`, que es quien la lee y la escribe; se
/// reexporta acá porque esta es la capa que la hace cumplir.
pub use crate::vecstore::Identidad;

/// El directorio donde viven los paquetes de una base.
pub fn directorio(dir: &Path) -> PathBuf {
    dir.join("paquetes")
}

/// La ruta canónica del paquete llamado `nombre`.
pub fn ruta(dir: &Path, nombre: &str) -> PathBuf {
    directorio(dir).join(format!("{nombre}.{EXTENSION}"))
}

/// El resultado de resolver el paquete activo. **No es un `Result`**: la
/// capa vectorial no tiene permitido hacer fallar a quien busca. Todo lo que
/// sale mal se convierte en `pack: None` más un aviso por stderr.
pub struct Activo {
    pub pack: Option<Pack>,
    /// La identidad con la que la base tenía registrado a `pack` cuando se
    /// lo abrió. Es `Some` exactamente cuando `pack` lo es.
    ///
    /// Viaja al lado del paquete porque escribir vectores la exige:
    /// `vecstore::sincronizar` tiene que poder comprobar, dentro de la
    /// transacción en la que escribe, que sigue siendo la registrada.
    /// Buscar no la necesita —leer con el paquete equivocado da un mal
    /// resultado, no un índice mezclado—, así que `resolver_y_avisar` la
    /// descarta y los caminos de lectura ni la ven.
    pub identidad: Option<Identidad>,
    /// Qué contarle al usuario, si hay algo que contarle. Sin paquete
    /// instalado es siempre `None`: quien no usa la capa no lee avisos.
    pub aviso: Option<String>,
}

impl Activo {
    fn sin_paquete() -> Activo {
        Activo { pack: None, identidad: None, aviso: None }
    }

    fn degradado(aviso: String) -> Activo {
        Activo { pack: None, identidad: None, aviso: Some(aviso) }
    }

    fn resuelto(pack: Pack, identidad: Identidad, aviso: Option<String>) -> Activo {
        Activo { pack: Some(pack), identidad: Some(identidad), aviso }
    }

    /// Deja el aviso en stderr, si lo hay.
    pub fn avisar(&self) {
        if let Some(a) = &self.aviso {
            eprintln!("{a}");
        }
    }

    /// El par que necesita quien va a **escribir** vectores. `None` si no
    /// hay paquete utilizable, que es cuando no hay nada que escribir.
    pub fn para_escribir(&self) -> Option<(&Pack, &Identidad)> {
        match (&self.pack, &self.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        }
    }
}

/// Abre el paquete registrado, si lo hay, dejando la base consistente.
///
/// Devuelve `None` —degradando a bm25— cuando no hay paquete registrado, el
/// archivo desapareció, está corrupto, o la reconciliación no se pudo hacer:
/// buscar nunca puede fallar por culpa de la capa vectorial.
///
/// Si el paquete que hay en disco no es el que registró el índice (otro
/// nombre, u otras dimensiones), reindexa antes de devolverlo. Y si no puede
/// reindexar, no devuelve el paquete: buscar con los vectores viejos y el
/// paquete nuevo sería exactamente la mezcla que no puede pasar.
pub fn resolver(conn: &Connection, dir: &Path) -> Activo {
    match resolver_interno(conn, dir) {
        Ok(a) => a,
        Err(e) => Activo::degradado(format!(
            "aviso: no se pudo resolver el paquete de vocabulario ({e}); \
             se busca solo con bm25."
        )),
    }
}

/// `resolver` más el aviso ya impreso, que es lo que quiere casi todo el
/// mundo que la llama.
pub fn resolver_y_avisar(conn: &Connection, dir: &Path) -> Option<Pack> {
    let a = resolver(conn, dir);
    a.avisar();
    a.pack
}

fn resolver_interno(conn: &Connection, dir: &Path) -> Result<Activo> {
    let Some((nombre, sha, dims)) = vecstore::pack_registrado(conn)? else {
        // El camino de quien no instaló nada: una consulta y afuera.
        return Ok(Activo::sin_paquete());
    };
    // Una instalación pudo haber muerto entre el rename —que ya reemplazó el
    // archivo— y el `adoptar_pack` que iba a describirlo. La marca es la
    // única señal de eso: el sha registrado se descarta a propósito, así que
    // si el build nuevo se llama igual y mide igual que el viejo —actualizar
    // `en-es`, el caso más común— la guarda de identidad no lo ve.
    //
    // Solo es peligrosa cuando nombra al paquete **registrado**: el rename
    // escribe en `paquetes/{nombre_nuevo}`, de modo que si el nombre es otro,
    // el archivo del registrado no lo tocó nadie y la marca quedó huérfana.
    //
    // Esta consulta no la paga quien no instaló nada: sin paquete registrado
    // ya salimos arriba.
    let sospechoso = match vecstore::instalacion_pendiente(conn)? {
        None => false,
        Some(instalando) if instalando == nombre => true,
        Some(_) => {
            let _ = vecstore::limpiar_instalacion(conn);
            false
        }
    };
    // El sha no se verifica acá: hashear 22 MB en cada arranque penalizaría
    // a quien sí instaló el paquete. Se verifica una sola vez, al instalar.
    let ruta_registrada = ruta(dir, &nombre);
    if !ruta_registrada.exists() {
        return Ok(Activo::degradado(format!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            ruta_registrada.display()
        )));
    }
    let p = match Pack::abrir(&ruta_registrada) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Activo::degradado(format!(
                "aviso: el paquete {nombre} en {} no se pudo abrir ({e}); \
                 se busca solo con bm25. Reinstalalo con `mem model install`.",
                ruta_registrada.display()
            )))
        }
    };
    if !sospechoso && p.nombre() == nombre && p.dims() == dims {
        return Ok(Activo::resuelto(p, (nombre, sha, dims), None));
    }
    // Reconciliar recalcula el sha, vuelve a adoptar —lo que borra los
    // vectores viejos y la marca en una transacción— y reindexa. Para una
    // instalación interrumpida antes del rename es trabajo de más, pero es
    // trabajo de una sola vez y del lado seguro: la alternativa es no poder
    // distinguir el caso en el que el archivo sí cambió.
    let aviso = if sospechoso {
        format!(
            "aviso: una instalación de {nombre} quedó a medias y el archivo en disco puede \
             no ser el que indexó esta base; reindexando para no mezclar vectores."
        )
    } else {
        format!(
            "aviso: el paquete en disco ({}, {} dimensiones) no es el que indexó esta base \
             ({nombre}, {dims} dimensiones); reindexando para no mezclar vectores.",
            p.nombre(),
            p.dims()
        )
    };
    match reconciliar(conn, dir, &ruta_registrada, p) {
        Ok((p, id)) => Ok(Activo::resuelto(p, id, Some(aviso))),
        Err(e) => Ok(Activo::degradado(format!(
            "{aviso}\naviso: no se pudo reindexar ({e}); se busca solo con bm25 \
             hasta que se pueda. Reinstalá el paquete con `mem model install`."
        ))),
    }
}

/// Hace que la base vuelva a describir al paquete que está en disco.
///
/// Lo delicado es el orden. El rename va **primero**, a propósito: si
/// `paquetes/` es de solo lectura —el caso que de verdad pasa— falla antes
/// de tocar la base y no queda nada a medias; la búsqueda degrada a bm25 y
/// se recupera sola cuando el directorio vuelva a ser escribible. Si el
/// rename sale bien pero la base no acompaña, se deshace: el registro no
/// cambió, así que la convención ruta↔nombre tiene que seguir valiendo para
/// el nombre viejo o la próxima corrida no encontraría el archivo nunca más.
fn reconciliar(
    conn: &Connection,
    dir: &Path,
    ruta_actual: &Path,
    p: Pack,
) -> Result<(Pack, Identidad)> {
    // El resto del código asume la convención ruta↔nombre. Si el nombre real
    // (leído del archivo) es otro, hay que renombrarlo como parte del
    // reindexado. En POSIX alcanzaría con mover el mmap abierto sin más
    // —el inodo es el mismo—, pero Windows no deja renombrar un archivo que
    // tenga un mapeo de memoria abierto. Por eso `p` se suelta antes del
    // rename y se reabre después desde `destino`: ninguna plataforma se
    // queda con un mmap apuntando al nombre viejo mientras el archivo se
    // mueve.
    let nombre = p.nombre().to_string();
    let dims = p.dims();
    let destino = ruta(dir, &nombre);
    let hubo_rename = destino != ruta_actual;
    let p = if hubo_rename {
        drop(p);
        std::fs::rename(ruta_actual, &destino).with_context(|| {
            format!(
                "no se pudo renombrar {} a {} al reindexar",
                ruta_actual.display(),
                destino.display()
            )
        })?;
        match Pack::abrir(&destino) {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::rename(&destino, ruta_actual);
                return Err(e).with_context(|| {
                    format!(
                        "el rename a {} salió bien pero el paquete no se pudo reabrir",
                        destino.display()
                    )
                });
            }
        }
    } else {
        p
    };
    let adopcion = pack::sha256(&destino).and_then(|sha| {
        vecstore::adoptar_pack(conn, &nombre, &sha, dims)?;
        Ok((nombre.clone(), sha, dims))
    });
    let identidad = match adopcion {
        Ok(i) => i,
        Err(e) => {
            if hubo_rename {
                // `p` tiene mapeado `destino`: hay que soltarlo antes de
                // deshacer el rename, por la misma razón de Windows que
                // obligó a reabrirlo más arriba.
                drop(p);
                let _ = std::fs::rename(&destino, ruta_actual);
            }
            return Err(e);
        }
    };
    // Rellenar los vectores es idempotente y va fuera de la parte atómica: si
    // el proceso muere acá quedan menos vectores de los debidos —que la
    // próxima escritura completa— pero nunca vectores de dos paquetes.
    //
    // Si otro proceso se adueñó del paquete en el medio, este reindexado ya
    // no vale y devolver `p` sería entregarle a quien busca un paquete que
    // no es el registrado. Se falla, que en `resolver_interno` se traduce en
    // degradar a bm25 esta vez; la próxima resolución encuentra el paquete
    // nuevo y arranca limpia.
    if vecstore::sincronizar(conn, &p, &identidad)?.abandonada() {
        bail!(
            "otro proceso cambió el paquete de vocabulario mientras se reindexaba con {}",
            nombre
        );
    }
    Ok((p, identidad))
}

/// Lo que `install` le cuenta a quien lo corrió.
pub struct Instalacion {
    pub nombre: String,
    pub dims: usize,
    pub fragmentos: usize,
    /// `true` si otro proceso cambió el paquete registrado mientras esta
    /// instalación indexaba: el paquete quedó instalado en disco pero ya no
    /// es el que la base usa, y el índice quedó a medias.
    pub desplazada: bool,
}

/// Reemplaza `destino` con el contenido de `origen`, incluso si `destino`
/// ya existe.
///
/// `std::fs::rename` reemplaza atómicamente sobre POSIX cuando el destino
/// ya existe, pero en Windows falla en ese caso. Acá se hace en dos pasos,
/// sin depender de que el motivo del fallo en Windows sea reconocible desde
/// otra plataforma:
///
/// 1. Si `destino` es un archivo, se lo aparta a un nombre lateral en el
///    mismo directorio (mismo filesystem: el rename que lo aparta y el que
///    lo restaura, si hace falta, son igual de baratos que el original).
/// 2. Se renombra `origen` a `destino`.
/// 3. Si el paso 2 sale bien, se borra el lateral. Si sale mal —el
///    antivirus reteniendo el archivo un instante, por ejemplo: un error
///    real, no un cuelgue— se **restaura** el lateral a `destino` antes de
///    devolver el error, así que quien llame encuentra exactamente el
///    archivo que ya funcionaba, no un destino borrado y sin reemplazo.
///
/// Un primer intento de este helper borraba `destino` antes de renombrar
/// en vez de apartarlo: si el segundo paso fallaba, el archivo viejo —que
/// hasta ese momento andaba— quedaba perdido sin necesidad. Detectable
/// (`resolver_interno` ve que falta y degrada a bm25), pero una regresión
/// de resiliencia real frente al rename atómico que hacía POSIX y que este
/// helper reemplaza: nunca hacía falta perder un archivo que servía.
///
/// Cuando `destino` existe pero no es un archivo (un directorio, un estado
/// ya corrupto que ninguna instalación produce) no hay nada bueno que
/// resguardar, así que se renombra directo y el error —si lo hay— se
/// propaga tal cual, sin abrir la ventana del paso 1.
///
/// Sigue quedando una ventana breve, entre el paso 1 y el paso 2, en la que
/// `destino` no existe. Es degradación aceptable y no silenciosa: una
/// búsqueda concurrente que mire justo ahí ve que falta el archivo
/// (`resolver_interno` lo comprueba con `ruta_registrada.exists()`) y cae a
/// bm25, que es el resultado seguro; y si el proceso muere en esa ventana,
/// la marca `pack_instalando` ya cubre que la base pueda describir un
/// paquete que el disco no tiene. Lo que nunca puede pasar —que la base
/// describa un paquete y el disco tenga otro sin que nada lo note— no
/// depende de esta ventana: lo cubre que `destino` sea, en todo momento,
/// "el archivo de `nombre`" o directamente no exista.
fn reemplazar(origen: &Path, destino: &Path) -> std::io::Result<()> {
    if !destino.is_file() {
        return std::fs::rename(origen, destino);
    }
    let lateral = destino.with_file_name(format!(
        "{}.reemplazando-{}",
        destino.file_name().and_then(|n| n.to_str()).unwrap_or("paquete"),
        std::process::id()
    ));
    std::fs::rename(destino, &lateral)?;
    match std::fs::rename(origen, destino) {
        Ok(()) => {
            // Best-effort: si esto falla queda un archivo lateral sobrante
            // tirado, no un estado inconsistente — `destino` ya tiene el
            // contenido correcto.
            let _ = std::fs::remove_file(&lateral);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&lateral, destino);
            Err(e)
        }
    }
}

/// Instala un paquete desde un archivo local y reindexa.
///
/// Es la única entrada que cambia el paquete de una base, y está escrita
/// para que no exista un estado intermedio observable que importe:
///
/// 1. Se verifica el sha256 —acá y solo acá— y que el paquete abra.
/// 2. Se descarta si no reconocería ni una palabra de esta memoria.
/// 3. El archivo se copia a un temporal y recién ahí se renombra sobre el
///    destino, que dentro del mismo directorio es atómico. Por eso instalar
///    desde la ruta ya instalada —reinstalar "para forzar el reindexado"—
///    funciona en vez de truncar el paquete a cero bytes.
/// 4. Antes del rename se anota `pack_instalando`, porque el rename ya
///    reemplaza el archivo y el registro se escribe recién en el paso
///    siguiente: entre los dos, un SIGKILL dejaría el build nuevo en disco
///    con `meta` describiendo al viejo, y con el mismo nombre y las mismas
///    dimensiones nada lo delataría. La marca la borra `adoptar_pack` en su
///    propia transacción, así que sobrevivir a la instalación significa
///    exactamente "el archivo puede no ser el que `meta` describe" y
///    `resolver` reindexa.
/// 5. Borrar los vectores viejos y anotar el paquete nuevo es **una sola
///    transacción**: no hay ninguna ventana con vectores de un paquete
///    atribuidos a otro.
/// 6. Rellenar los vectores queda afuera, por lotes y es idempotente: un
///    Ctrl-C en el medio deja menos vectores, nunca vectores mezclados.
pub fn instalar(conn: &Connection, dir: &Path, archivo: &Path) -> Result<Instalacion> {
    let sha = pack::sha256(archivo)?;
    verificar_firma(archivo, &sha)?;

    let (nombre, dims, sirve) = {
        let p = Pack::abrir(archivo)?;
        (p.nombre().to_string(), p.dims(), !solo_vectores_nulos(conn, &p)?)
    };
    if !sirve {
        bail!(
            "el paquete {nombre} no reconoce ni una palabra de esta memoria: todos los \
             vectores saldrían nulos y la búsqueda vectorial no aportaría nada. No se \
             instaló nada; revisá que sea el paquete del idioma correcto."
        );
    }

    let destino_dir = directorio(dir);
    std::fs::create_dir_all(&destino_dir)?;
    let destino = ruta(dir, &nombre);
    let temporal = destino_dir.join(format!(".instalando-{}.{EXTENSION}", std::process::id()));
    let copia = (|| -> Result<()> {
        std::fs::copy(archivo, &temporal).with_context(|| {
            format!("no se pudo copiar {} a {}", archivo.display(), temporal.display())
        })?;
        // Que la copia sea la copia: si quedó corta o el disco se llenó, el
        // sha no da y el paquete instalado no llega a existir.
        let sha_copia = pack::sha256(&temporal)?;
        if sha_copia != sha {
            bail!("la copia de {} quedó incompleta; no se instaló nada", archivo.display());
        }
        Pack::abrir(&temporal)?;
        Ok(())
    })();
    if let Err(e) = copia {
        let _ = std::fs::remove_file(&temporal);
        return Err(e);
    }
    // La marca va antes del rename, no después: si se escribiera después, el
    // tramo sin cubrir sería justo el que se quiere cubrir. Que sobre —que
    // quede puesta sin que el archivo haya cambiado— cuesta un reindexado;
    // que falte cuesta un índice mezclado en silencio.
    vecstore::marcar_instalacion(conn, &nombre)?;
    if let Err(e) = reemplazar(&temporal, &destino) {
        let _ = std::fs::remove_file(&temporal);
        // `reemplazar` restaura `destino` a lo que tenía antes cuando el
        // segundo paso falla, así que el archivo del paquete registrado no
        // se tocó (a menos que la restauración *también* fallara, un doble
        // fallo improbable; ahí directamente falta, y eso ya lo detecta
        // `resolver_interno` sin necesitar la marca —busca
        // `ruta_registrada.exists()` antes de mirarla—). Dejarla puesta
        // solo costaría un reindexado inútil la próxima vez que el nombre
        // coincida con el registrado.
        let _ = vecstore::limpiar_instalacion(conn);
        return Err(anyhow::Error::new(e)
            .context(format!("no se pudo instalar el paquete en {}", destino.display())));
    }

    // Borra la marca en la misma transacción en la que adopta.
    vecstore::adoptar_pack(conn, &nombre, &sha, dims)?;
    let p = Pack::abrir(&destino)?;
    // Indexar un corpus grande lleva minutos y otra terminal puede instalar
    // otro paquete en el medio. Si pasa, esta instalación queda desplazada:
    // no es un error —el paquete se instaló y el otro proceso está
    // indexando el suyo— pero hay que decirlo, porque el índice quedó
    // incompleto y `model status` es el único lugar donde eso se ve.
    let s = vecstore::sincronizar(conn, &p, &(nombre.clone(), sha, dims))?;
    Ok(Instalacion { nombre, dims, fragmentos: s.escritos(), desplazada: s.abandonada() })
}

/// Quita el paquete y sus vectores. Devuelve el nombre del que había, o
/// `None` si no había ninguno.
///
/// Primero la base y después el archivo: si borrar el archivo falla, la base
/// ya quedó sin paquete y la búsqueda vuelve a ser solo bm25, que es lo que
/// se pidió. Al revés quedaría un registro apuntando a la nada.
pub fn quitar(conn: &Connection, dir: &Path) -> Result<Option<String>> {
    let Some((nombre, _, _)) = vecstore::pack_registrado(conn)? else {
        return Ok(None);
    };
    vecstore::olvidar_pack(conn)?;
    let _ = std::fs::remove_file(ruta(dir, &nombre));
    Ok(Some(nombre))
}

/// Verifica el `.sha256` que viaja al lado del paquete, si viaja alguno.
fn verificar_firma(archivo: &Path, sha: &str) -> Result<()> {
    let firma = archivo.with_file_name(format!(
        "{}.sha256",
        archivo.file_name().unwrap_or_default().to_string_lossy()
    ));
    if !firma.exists() {
        return Ok(());
    }
    let crudo = std::fs::read_to_string(&firma)?;
    let esperado = crudo.split_whitespace().next().unwrap_or("");
    if esperado != sha {
        bail!(
            "el sha256 no coincide: el archivo da {sha} y {} dice {esperado}",
            firma.display()
        );
    }
    Ok(())
}

/// ¿El paquete produce el vector nulo para todo lo que hay guardado?
///
/// Pasa cuando la tabla del paquete no cubre el vocabulario que usa esta
/// memoria: instala limpio, reporta cobertura completa y no encuentra nada
/// nunca, sin ninguna señal. Con la memoria vacía no hay nada que juzgar y
/// se deja pasar.
fn solo_vectores_nulos(conn: &Connection, p: &Pack) -> Result<bool> {
    let cuerpos: Vec<String> = conn
        .prepare("select body from chunks where trim(body) <> '' limit ?1")?
        .query_map([MUESTRA as i64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if cuerpos.is_empty() {
        return Ok(false);
    }
    for c in &cuerpos {
        if p.embeber(c)?.iter().any(|&x| x != 0.0) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// El paquete activo, revalidado antes de cada operación.
///
/// El servidor MCP vive horas. En ese rato otra terminal puede correr
/// `mem model install` o `mem model remove`, y un servidor que siguiera
/// usando el paquete que abrió al arrancar escribiría vectores del paquete
/// viejo junto a los del nuevo. Con dimensiones distintas el blob corto
/// quedaría muerto; con las mismas —el caso real, porque el formato fija 256
/// para todo paquete— nada los distinguiría y la búsqueda calcularía cosenos
/// contra dos espacios vectoriales mezclados.
///
/// Revalidar cuesta una consulta a `meta`. Reabrir —que cuesta entre 40 y
/// 120 ms, y unos 54 MB de heap del tokenizador— pasa solo cuando la
/// identidad cambió de verdad. `nuevo` no abre nada: arranca en blanco
/// (`registro: None, activo: None`), a propósito, para que una sesión que
/// nunca busca ni guarda no pague nunca ese heap. Quien quiera el paquete
/// llama a `actual`, que abre en el momento en que de verdad hace falta —la
/// primera búsqueda o el primer guardado— y no antes.
/// Avisa, **sin abrir el paquete**, si el registro apunta a un archivo que no
/// está. Existe porque abrir es caro y se difiere hasta el primer uso: una
/// sesión que solo lista tags nunca abriría nada y por lo tanto nunca vería
/// que su paquete desapareció. Comprobar que el archivo exista cuesta un
/// `stat`, así que ese caso —el frecuente— se puede detectar al arrancar.
///
/// Un paquete que existe pero está corrupto sigue descubriéndose recién al
/// usarlo: detectarlo exige parsearlo, que es justo lo que se difiere.
pub fn avisar_si_falta_el_archivo(conn: &Connection, dir: &Path) {
    let Ok(Some((nombre, _, _))) = vecstore::pack_registrado(conn) else {
        return;
    };
    let r = ruta(dir, &nombre);
    if !r.exists() {
        eprintln!(
            "aviso: el paquete {nombre} está registrado pero falta {}; \
             se busca solo con bm25. Reinstalalo con `mem model install`.",
            r.display()
        );
    }
}

pub struct PaqueteVivo {
    dir: PathBuf,
    /// Lo que decía `meta` la última vez que se recargó, se haya podido
    /// abrir el paquete o no, o `None` si todavía no se recargó nunca. Es la
    /// señal de "cambió algo" (o "no se miró todavía"): guardarla también
    /// cuando el paquete no abre evita reintentar abrirlo —entre 40 y 120
    /// ms— en cada mensaje mientras siga roto.
    registro: Option<Identidad>,
    /// El paquete abierto junto con la identidad con la que se lo abrió.
    /// Nunca se arman por separado: ese par es lo que `sincronizar`
    /// necesita para no escribir vectores de un paquete caducado, y leer
    /// `meta` una segunda vez para completarlo reabriría la misma ventana.
    activo: Option<(Pack, Identidad)>,
}

impl PaqueteVivo {
    /// No toca la base ni el disco: abrir el paquete queda para el primer
    /// `actual()` que de verdad lo necesite. Ver el comentario de
    /// `PaqueteVivo` arriba.
    pub fn nuevo(dir: &Path) -> PaqueteVivo {
        PaqueteVivo { dir: dir.to_path_buf(), registro: None, activo: None }
    }

    /// El paquete que corresponde usar ahora mismo, con la identidad bajo
    /// la que se lo abrió.
    ///
    /// Los dos salen juntos de la misma llamada a propósito: leerlos por
    /// separado reabriría la ventana que este par cierra, porque entre una
    /// lectura y la otra el registro puede cambiar.
    pub fn actual(&mut self, conn: &Connection) -> Option<(&Pack, &Identidad)> {
        let ahora = vecstore::pack_registrado(conn).ok().flatten();
        if ahora != self.registro {
            self.recargar(conn);
        }
        self.activo.as_ref().map(|(p, i)| (p, i))
    }

    fn recargar(&mut self, conn: &Connection) {
        let a = resolver(conn, &self.dir);
        a.avisar();
        // El par sale entero de `resolver`, que sabe con qué identidad abrió
        // el paquete. Releer `meta` acá para completarlo podría emparejar el
        // paquete de antes con el registro de después.
        self.activo = match (a.pack, a.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        };
        // Para detectar el próximo cambio, en cambio, sirve lo que dice
        // `meta` ahora: `resolver` puede haber reindexado y cambiado el
        // registro, y si se guardara el de antes se recargaría en cada
        // mensaje.
        self.registro = vecstore::pack_registrado(conn).ok().flatten();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokenizador Unigram de una sola pieza. Estos tests no prueban
    /// `embeber`, solo que el paquete abre y que `reconciliar`/`reemplazar`
    /// mueven el archivo correcto; una pieza alcanza.
    const TOKENIZER_MINIMO: &str = r#"{
     "version":"1.0","truncation":null,"padding":null,"added_tokens":[],
     "normalizer":null,
     "pre_tokenizer":{"type":"Metaspace","replacement":"▁","prepend_scheme":"always","split":true},
     "post_processor":null,"decoder":null,
     "model":{"type":"Unigram","unk_id":0,"byte_fallback":false,
       "vocab":[["<unk>",0.0]]}
    }"#;

    /// Escribe un paquete ANPK mínimo válido. Duplica a mano el formato que
    /// `tests/apoyo::escribir_pack_filas` ya sabe escribir porque ese
    /// helper vive en el crate de integración: los tests unitarios de este
    /// módulo, que necesitan tocar `reconciliar` por ser privada, no pueden
    /// importarlo.
    fn escribir_pack_de_prueba(ruta: &Path, nombre: &str) {
        let dims = 4usize;
        let fila = [0u8, 0, 0, 0];
        let tok = TOKENIZER_MINIMO.as_bytes();
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"ANPK");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(dims as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes()); // n_tokens
        b.extend_from_slice(&(nombre.len() as u32).to_le_bytes());
        b.extend_from_slice(nombre.as_bytes());
        b.extend_from_slice(&(tok.len() as u64).to_le_bytes());
        b.extend_from_slice(tok);
        for _ in 0..dims {
            b.extend_from_slice(&1.0f32.to_le_bytes()); // escalas
        }
        for _ in 0..dims {
            b.extend_from_slice(&0.0f32.to_le_bytes()); // mínimos
        }
        b.extend_from_slice(&fila);
        std::fs::write(ruta, b).unwrap();
    }

    // --- bloqueo 2: reemplazar sobre un destino que ya existe -------------
    //
    // `std::fs::rename` reemplaza atómicamente un destino existente en
    // POSIX, así que el primer test de este grupo ya pasaba con un rename
    // directo. Lo que prueba es que `reemplazar` —la función nueva,
    // portable, que aparta el destino en vez de borrarlo— hace lo mismo: es
    // justo la operación (rename sobre un destino ya ocupado) que hoy no
    // sería portable a Windows si se dejara como estaba. El tercer test
    // ejercita la parte que un rename atómico nunca necesitó: recuperarse
    // cuando el segundo paso falla de verdad.

    /// Ningún archivo lateral (`*.reemplazando-*`) sobrevive a un
    /// `reemplazar`, salga bien o mal.
    fn sin_laterales(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .all(|n| !n.contains(".reemplazando-"))
    }

    #[test]
    fn reemplazar_sobre_destino_existente_deja_el_contenido_del_origen() {
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen");
        let destino = d.path().join("destino");
        std::fs::write(&origen, b"contenido nuevo").unwrap();
        std::fs::write(&destino, b"contenido viejo, mas largo que el nuevo").unwrap();

        reemplazar(&origen, &destino).unwrap();

        assert!(!origen.exists(), "el origen tiene que haberse movido, no copiado");
        assert_eq!(std::fs::read(&destino).unwrap(), b"contenido nuevo");
        assert!(sin_laterales(d.path()), "no puede quedar un archivo lateral sobrante");
    }

    #[test]
    fn reemplazar_sin_destino_previo_tambien_funciona() {
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen");
        let destino = d.path().join("destino");
        std::fs::write(&origen, b"contenido").unwrap();

        reemplazar(&origen, &destino).unwrap();

        assert_eq!(std::fs::read(&destino).unwrap(), b"contenido");
    }

    #[test]
    fn reemplazar_si_el_segundo_rename_falla_el_destino_viejo_sigue_sirviendo() {
        // El origen no existe, así que el primer rename (destino -> lateral)
        // sale bien pero el segundo (origen -> destino) falla con un error
        // real — no un cuelgue del proceso, el mismo tipo de fallo que
        // dejaría un antivirus reteniendo el archivo un instante. Antes de
        // este arreglo, un `reemplazar` que llegara hasta acá ya había
        // borrado el destino: el archivo viejo, que hasta ese momento
        // andaba, se perdía sin necesidad. Ahora tiene que seguir estando.
        let d = tempfile::tempdir().unwrap();
        let origen = d.path().join("origen"); // deliberadamente no se crea
        let destino = d.path().join("destino");
        std::fs::write(&destino, b"contenido viejo que todavia funciona").unwrap();

        let err = reemplazar(&origen, &destino).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        assert_eq!(
            std::fs::read(&destino).unwrap(),
            b"contenido viejo que todavia funciona",
            "el destino que funcionaba antes de intentar el reemplazo tiene que seguir ahí"
        );
        assert!(sin_laterales(d.path()), "no puede quedar un archivo lateral sobrante");
    }

    // --- bloqueo 3: reconciliar con el mmap todavía abierto ----------------

    #[test]
    fn reconciliar_renombra_un_archivo_con_el_mmap_todavia_abierto() {
        // Reproduce la forma exacta del problema de Windows: abrir el Pack
        // —que mapea el archivo en memoria— y pasar ESE mismo `Pack`, en el
        // mismo proceso, a `reconciliar` para que renombre el archivo que
        // tiene mapeado. Es lo que hace `resolver_interno` en producción
        // cuando el paquete en disco no es el registrado.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        std::fs::create_dir_all(directorio(dir)).unwrap();
        let ruta_vieja = ruta(dir, "viejo-es");
        // El archivo vive bajo el nombre "viejo-es" pero declara ser
        // "nuevo-es": exactamente el caso que obliga a reconciliar a
        // renombrarlo.
        escribir_pack_de_prueba(&ruta_vieja, "nuevo-es");

        let conn = crate::db::open(&dir.join("mem.db")).unwrap();
        vecstore::registrar_pack(&conn, "viejo-es", "sha-viejo", 4).unwrap();

        let p = Pack::abrir(&ruta_vieja).unwrap(); // mmap abierto sobre ruta_vieja

        let (p2, identidad) = reconciliar(&conn, dir, &ruta_vieja, p)
            .expect("reconciliar tiene que poder renombrar el archivo que tiene mapeado");

        assert_eq!(identidad.0, "nuevo-es");
        assert_eq!(p2.nombre(), "nuevo-es");
        assert!(!ruta_vieja.exists(), "el archivo con el nombre viejo tiene que haberse ido");
        assert!(
            ruta(dir, "nuevo-es").exists(),
            "el archivo tiene que vivir ahora en el nombre nuevo"
        );
    }
}
