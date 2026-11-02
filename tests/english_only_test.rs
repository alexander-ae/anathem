//! No queda español en lo que anathem imprime.
//!
//! Es el modo de falla que esta migración produjo tres veces, y las tres
//! fueron la misma forma: una capa traducida envolviendo otra que no. Un
//! aviso del servidor con la primera línea en inglés y el resto en español;
//! `recent_memories` imprimiendo `[tema:` mientras las otras dos tools
//! decían `[topic:`; un `Not saved:` con el cuerpo del error en español.
//! Ninguna la encontró quien escribió el cambio.
//!
//! Mira solo cadenas y saltea comentarios: los comentarios del repositorio
//! ya están en inglés por otra vía, y mezclar las dos cosas haría fallar el
//! guardia por razones que no son las suyas.

/// Palabras que no existen en inglés y sí en el español que había. Son
/// deliberadamente comunes: lo que se quiere cazar es una frase entera que
/// se quedó sin traducir, no una palabra suelta.
const TELLTALE: &[&str] = &[
    "memoria", "búsqueda", "guardar", "guardada", "archivo", "paquete",
    "clave", "proyecto", "ventana", "tema", "ninguna", "ningún",
    "no se pudo", "no existe", "está", "tiene", "hay que", "según",
];

/// `tags.rs` lleva los patrones de la taxonomía por defecto, que son regex
/// pensadas para reconocer texto en español dentro de las memorias. Son
/// datos, no mensajes: traducirlas cambiaría qué se etiqueta.
const EXEMPT: &[&str] = &["src/tags.rs"];

/// Cadenas en español que se quedan, cada una por una razón que no es
/// pereza. Van aparte de `EXEMPT` a propósito: eximir un archivo entero
/// esconde lo que venga después, y estas son pocas y conocidas.
const ALLOWED: &[(&str, &str)] = &[
    // Compatibilidad: son los nombres de clave que tenía `config.json`
    // antes de 0.2.1 y que `serde(alias)` sigue aceptando. Traducirlos
    // dejaría de leer los archivos que la gente ya tiene.
    ("traspaso_items", "alias de config.json anterior a 0.2.1"),
    ("traspaso_ventana", "alias de config.json anterior a 0.2.1"),
    ("soltar_paquete_tras", "alias de config.json anterior a 0.2.1"),
    // Migración: son los nombres de columna que la base tuvo y que
    // `rename_columns_to_english` busca para renombrarlos. Si cambian, una
    // base vieja deja de migrarse.
    ("clave", "nombre de columna que la migración busca"),
    ("valor", "nombre de columna que la migración busca"),
    ("agente", "nombre de columna que la migración busca"),
    ("creado_ts", "nombre de columna que la migración busca"),
    // Dato almacenado: es el valor que va a `memories.project` cuando no se
    // puede derivar uno. Cambiarlo partiría en dos los proyectos de
    // cualquier base existente, lo cual es una migración de datos y no una
    // traducción.
    ("sin-proyecto", "valor guardado en memories.project"),
    // Delimitadores del bloque de traspaso. Son marcadores, no prosa, y
    // cambiarlos no mejora nada para quien los lee: el modelo solo tiene
    // que verlos aparecer una vez.
    ("<<<memorias-anathem>>>", "delimitador del bloque de datos"),
    ("<<<fin-memorias-anathem>>>", "delimitador del bloque de datos"),
    // Restos de una instalación interrumpida: el nombre del archivo lateral
    // que `replace_file` crea. `doctor` los busca por este nombre, así que
    // cambiarlo dejaría sin detectar los que ya estén en disco.
    (".reemplazando-", "nombre de archivo que doctor busca"),
    // El directorio donde viven los paquetes, en disco: `~/.anathem/paquetes/`.
    // Renombrarlo dejaría a toda instalación existente sin encontrar su
    // paquete, así que es una migración de archivos y no una traducción. El
    // fallback de `replace_file` lo acompaña por la misma razón.
    ("paquetes", "directorio en disco de las instalaciones existentes"),
    ("paquete", "fallback del nombre de archivo lateral"),
];

/// Las partes de `text` que están dentro de una cadena, con su número de
/// línea.
///
/// Recorre el archivo entero llevando cuenta de si está adentro de una
/// cadena, en vez de mirar cada línea por su cuenta. La primera versión de
/// esto hacía `find('"')` y `rfind('"')` sobre cada línea y devolvía nada
/// cuando no había dos: o sea que saltaba TODAS las líneas de una cadena
/// multilínea, que es justo donde vivía el español que quedaba. El guardia
/// pasaba en verde con cinco mensajes enteros sin traducir.
///
/// `'"'` se neutraliza antes de recorrer: es un literal de carácter y no
/// abre nada, pero el recorrido lo contaría como si abriera.
fn strings_in(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut part = String::new();
    let mut start = 0usize;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.replace("'\"'", "'Q'");
        if !inside && line.trim_start().starts_with("//") {
            continue;
        }
        let mut esc = false;
        for c in line.chars() {
            if esc {
                esc = false;
                if inside { part.push(c); }
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                if inside && !part.trim().is_empty() {
                    // Cierra: se emite este literal y no se mezcla con el
                    // siguiente de la misma línea.
                    out.push((start, std::mem::take(&mut part)));
                } else {
                    part.clear();
                    start = n + 1;
                }
                inside = !inside;
            } else if inside {
                part.push(c);
            }
        }
        // Una cadena multilínea sigue abierta: se guarda lo de esta línea
        // para que el mensaje señale dónde está, y se sigue acumulando.
        if inside && !part.trim().is_empty() {
            out.push((n + 1, std::mem::take(&mut part)));
        }
    }
    out
}

#[test]
fn no_spanish_left_in_any_user_facing_string() {
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir("src").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let as_str = p.to_string_lossy().replace('\\', "/");
        if EXEMPT.iter().any(|e| as_str.ends_with(e)) {
            continue;
        }
        for (n, s) in strings_in(&std::fs::read_to_string(&p).unwrap()) {
            // Coincidencia exacta y no `contains`: una entrada como «clave»
            // comparada con `contains` eximiría cualquier cadena futura que
            // tuviera esa palabra, y con ella las dieciocho delatoras. Las
            // tres que sí son fragmentos van aparte y dicen por qué.
            const FRAGMENTS: &[&str] = &[
                // Aparece embebido en el nombre del archivo lateral.
                ".reemplazando-",
                // Los delimitadores viajan dentro del texto de traspaso.
                "<<<memorias-anathem>>>",
                "<<<fin-memorias-anathem>>>",
            ];
            let t = s.trim();
            if ALLOWED.iter().any(|(a, _)| t == *a) || FRAGMENTS.iter().any(|f| s.contains(f)) {
                continue;
            }
            let lower = s.to_lowercase();
            for t in TELLTALE {
                if lower.contains(t) {
                    offenders.push(format!("{}:{} «{t}» en {}", as_str, n, s.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "quedó español en cadenas que ve el usuario:\n{}",
        offenders.join("\n")
    );
}

/// Lo mismo para los scripts. Un instalador a medio traducir es peor que uno
/// entero en español: quien lo corre ve media frase que entiende y media que
/// no, y no sabe si el problema es suyo.
///
/// Acá no hay cadenas que separar de los comentarios —todo el archivo es
/// texto— así que mira el archivo entero menos las líneas que son
/// claramente identificadores, y se apoya en que el español lleva acentos
/// con mucha más frecuencia que el inglés lleva caracteres no-ASCII.
#[test]
fn no_spanish_left_in_the_scripts() {
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir("scripts").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|x| x != "sh") {
            continue;
        }
        for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
            // Las banderas viejas siguen aceptándose en el `case`, a
            // propósito: son compatibilidad, no texto sin traducir.
            if line.contains("--compilar") || line.contains("--etiqueta")
                || line.contains("--paquete") || line.contains("--sin-paquete")
                || line.contains("--configurar-path") || line.contains("--conectar")
                || line.contains("--prefijo") || line.contains("--sin-verificar")
            {
                continue;
            }
            if line.chars().any(|c| "áéíóúñ¿¡Á É Í Ó Ú Ñ".contains(c) && !c.is_whitespace()) {
                offenders.push(format!("{}:{} {}", p.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "quedó español en los scripts:\n{}",
        offenders.join("\n")
    );
}
