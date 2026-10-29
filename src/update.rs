//! Comprobar si hay una versión más nueva publicada.
//!
//! Es un comando y no algo automático a propósito. anathem no habla con
//! ningún servidor mientras trabaja, y que lo hiciera solo —aunque fuera una
//! vez por día— convertiría cada arranque en una señal de que esta máquina
//! está usando la herramienta. Acá la consulta la pide una persona.
//!
//! Tampoco baja ni reemplaza el binario. Eso lo hace el instalador, que ya
//! sabe verificar el sha256 y dónde va cada cosa; duplicar esa lógica sería
//! mantener dos caminos que tienen que coincidir.

use anyhow::{bail, Context, Result};
use std::process::Command;

/// Dónde preguntar. Se puede apuntar a otro lado con `ANATHEM_REPO`, igual
/// que en el instalador, para quien tenga su propia copia del proyecto.
fn latest_release_api() -> String {
    let repo =
        std::env::var("ANATHEM_REPO").unwrap_or_else(|_| "__alexander__/anathem".to_string());
    let codificado = repo.replace('/', "%2F");
    format!("https://gitlab.com/api/v4/projects/{codificado}/releases/permalink/latest")
}

/// Baja una URL con lo que haya en el sistema.
///
/// Se usa `curl` o `wget` en vez de un cliente HTTP compilado adentro porque
/// esto se invoca a mano y muy de vez en cuando: meter una pila de TLS en el
/// binario para eso lo engordaría y sumaría dependencias que después hay que
/// seguir en cada aviso de seguridad.
fn fetch(url: &str) -> Result<String> {
    let intento = |programa: &str, args: &[&str]| -> Option<std::process::Output> {
        Command::new(programa).args(args).output().ok().filter(|o| o.status.success())
    };
    let salida = intento("curl", &["-fsSL", "--max-time", "20", url])
        .or_else(|| intento("wget", &["-q", "-O", "-", "--timeout=20", url]));
    match salida {
        Some(o) => String::from_utf8(o.stdout).context("la respuesta no es UTF-8"),
        None => bail!(
            "no pude consultar {url}.\n   \
             Hace falta `curl` o `wget`, y conexión. Si estás sin red, no pasa nada: \
             anathem funciona igual."
        ),
    }
}

/// La etiqueta de la última release, sin la `v`.
fn latest_published(json: &str) -> Result<String> {
    // Se busca el campo a mano para no sumar un parser de JSON a un camino
    // que corre una vez cada tanto; la respuesta es de GitLab y el campo es
    // estable.
    let marca = "\"tag_name\":\"";
    let i = json.find(marca).context("la respuesta no trae tag_name")?;
    let resto = &json[i + marca.len()..];
    let j = resto.find('"').context("tag_name sin cerrar")?;
    Ok(resto[..j].trim_start_matches('v').to_string())
}

/// Compara dos versiones tipo `0.2.0`. Devuelve si `publicada` es mayor.
///
/// Compara número a número y no como texto, porque `0.10.0` es mayor que
/// `0.9.0` pero menor alfabéticamente, y ese error aparece recién cuando el
/// proyecto llega a la décima versión.
pub fn is_newer(publicada: &str, actual: &str) -> bool {
    let partes = |s: &str| -> Vec<u64> {
        s.split(['.', '-']).map(|x| x.parse().unwrap_or(0)).collect()
    };
    let (a, b) = (partes(publicada), partes(actual));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Consulta y cuenta. Devuelve si hay una más nueva.
pub fn check_for_update() -> Result<bool> {
    let actual = env!("CARGO_PKG_VERSION");
    let json = fetch(&latest_release_api())?;
    let publicada = latest_published(&json)?;
    if is_newer(&publicada, actual) {
        println!("Hay una versión nueva: {publicada} (tenés la {actual}).");
        println!();
        println!("Para actualizar, el instalador hace lo mismo que la primera vez:");
        println!("    curl -fsSL https://gitlab.com/__alexander__/anathem/-/raw/main/scripts/instalar.sh -o instalar.sh");
        println!("    sh instalar.sh");
        println!();
        println!("Tus memorias no se tocan: viven en la base, no en el binario.");
        Ok(true)
    } else if publicada == actual {
        println!("Estás al día: {actual} es la última publicada.");
        Ok(false)
    } else {
        // Pasa en una copia compilada del repositorio, que va por delante de
        // lo publicado. Decir "estás al día" ahí sería mentir al revés.
        println!(
            "Tenés la {actual}, más nueva que la última publicada ({publicada}). \
             Seguramente la compilaste vos."
        );
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_lee_la_etiqueta_de_la_respuesta() {
        let j = r#"{"name":"v0.3.0","tag_name":"v0.3.0","description":"algo"}"#;
        assert_eq!(latest_published(j).unwrap(), "0.3.0");
        // Sin la `v` también.
        let j = r#"{"tag_name":"1.2.3"}"#;
        assert_eq!(latest_published(j).unwrap(), "1.2.3");
    }

    #[test]
    fn una_respuesta_sin_etiqueta_falla_y_lo_dice() {
        let e = latest_published(r#"{"message":"404 Not Found"}"#).unwrap_err().to_string();
        assert!(e.contains("tag_name"), "{e}");
    }

    #[test]
    fn las_versiones_se_comparan_por_numero_y_no_como_texto() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        // El caso que rompe la comparación alfabética.
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        // Iguales no es "más nueva".
        assert!(!is_newer("0.2.0", "0.2.0"));
        // Más vieja tampoco.
        assert!(!is_newer("0.1.0", "0.2.0"));
        // Distinta cantidad de partes.
        assert!(is_newer("0.2.1", "0.2"));
        assert!(!is_newer("0.2", "0.2.0"));
    }
}
