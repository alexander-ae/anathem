//! Checking whether a newer version has been published.
//!
//! This is a command and not something automatic, on purpose. anathem talks
//! to no server while it works, and doing so on its own — even once a day —
//! would turn every startup into a signal that this machine is using the
//! tool. Here a person asks for the lookup.
//!
//! It also neither downloads nor replaces the binary. The installer does
//! that, and it already knows how to verify the sha256 and where each piece
//! goes; duplicating that logic would mean maintaining two paths that have
//! to agree.

use anyhow::{bail, Context, Result};
use std::process::Command;

/// Where to ask. It can be pointed elsewhere with `ANATHEM_REPO`, same as in
/// the installer, for anyone running their own copy of the project.
fn latest_release_api() -> String {
    let repo =
        std::env::var("ANATHEM_REPO").unwrap_or_else(|_| "__alexander__/anathem".to_string());
    let codificado = repo.replace('/', "%2F");
    format!("https://gitlab.com/api/v4/projects/{codificado}/releases/permalink/latest")
}

/// Fetches a URL with whatever the system has.
///
/// It shells out to `curl` or `wget` instead of compiling an HTTP client in,
/// because this runs by hand and very occasionally: putting a TLS stack in
/// the binary for that would bloat it and add dependencies to track on every
/// security advisory.
fn fetch(url: &str) -> Result<String> {
    let intento = |programa: &str, args: &[&str]| -> Option<std::process::Output> {
        Command::new(programa).args(args).output().ok().filter(|o| o.status.success())
    };
    let out_text = intento("curl", &["-fsSL", "--max-time", "20", url])
        .or_else(|| intento("wget", &["-q", "-O", "-", "--timeout=20", url]));
    match out_text {
        Some(o) => String::from_utf8(o.stdout).context("la respuesta no es UTF-8"),
        None => bail!(
            "no pude consultar {url}.\n   \
             Hace falta `curl` o `wget`, y conexión. Si estás sin red, no pasa nada: \
             anathem funciona igual."
        ),
    }
}

/// The latest release's tag, without the leading `v`.
fn latest_published(json: &str) -> Result<String> {
    // The field is found by hand so a JSON parser is not added to a path
    // that runs once in a while; the response comes from GitLab and the
    // field is stable.
    let marca = "\"tag_name\":\"";
    let i = json.find(marca).context("la respuesta no trae tag_name")?;
    let resto = &json[i + marca.len()..];
    let j = resto.find('"').context("tag_name sin cerrar")?;
    Ok(resto[..j].trim_start_matches('v').to_string())
}

/// Compares two versions like `0.2.0`. Returns whether `published` is newer.
///
/// It compares number by number and not as text, because `0.10.0` is greater
/// than `0.9.0` but smaller alphabetically, and that bug only shows up once
/// the project reaches its tenth version.
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

/// Looks it up and reports. Returns whether a newer one exists.
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
        // Happens in a copy built from the repository, which runs ahead of
        // what is published. Saying "you are up to date" there would be
        // lying the other way around.
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
        // Without the leading `v` too.
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
        // The case that breaks alphabetical comparison.
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        // Equal is not "newer".
        assert!(!is_newer("0.2.0", "0.2.0"));
        // Older is not either.
        assert!(!is_newer("0.1.0", "0.2.0"));
        // Different number of parts.
        assert!(is_newer("0.2.1", "0.2"));
        assert!(!is_newer("0.2", "0.2.0"));
    }
}
