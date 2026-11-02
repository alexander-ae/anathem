//! Los documentos están en inglés y describen el programa que existe.
//!
//! La segunda mitad importa tanto como la primera. Traducir un documento no
//! sirve de nada si lo que dice ya no es cierto, y esta migración dejó justo
//! eso: `AGENTS.md` le promete a un agente que `mem doctor` termina en «Todo
//! en orden.», y hace tres commits que termina en «All good.». Un agente que
//! compruebe esa frase reporta que la instalación falló cuando está
//! perfecta.
//!
//! Este guardia se escribió ANTES de traducir, al revés que el de `src/`,
//! que se escribió al final y por eso no encontró lo que ya estaba roto.

use std::path::{Path, PathBuf};

/// Banderas y textos que el programa dejó de emitir o aceptar. Un documento
/// que los nombre está enseñando algo que no existe.
const GONE: &[(&str, &str)] = &[
    ("--configurar-path", "ahora --setup-path"),
    ("--conectar", "ahora --connect"),
    ("--sin-paquete", "ahora --no-pack"),
    ("--sin-verificar", "ahora --no-verify"),
    ("--prefijo", "ahora --prefix"),
    ("--compilar", "ahora --build"),
    ("--etiqueta", "ahora --tag"),
    ("Todo en orden", "ahora «All good.»"),
    ("--desde", "ahora --since (recent) o --from (import)"),
    ("--limite", "ahora --limit"),
    ("--todos", "ahora --all"),
    ("--fusionar", "ahora --merge"),
];

/// Registro y proceso, no documentación de uso: se quedan en español.
fn is_exempt(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    s.contains("docs/historia/") || s.contains("docs/superpowers/")
}

fn living_docs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("README.md"), PathBuf::from("AGENTS.md")];
    for e in std::fs::read_dir("docs").unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "md") && !is_exempt(&p) {
            v.push(p);
        }
    }
    v
}

#[test]
fn no_doc_teaches_something_that_no_longer_exists() {
    let mut offenders = Vec::new();
    for f in living_docs() {
        for (n, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            for (gone, now) in GONE {
                if line.contains(gone) {
                    offenders.push(format!("{}:{} «{gone}» — {now}", f.display(), n + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "documentos que enseñan algo que ya no existe:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn no_spanish_left_in_the_docs() {
    let mut offenders = Vec::new();
    for f in living_docs() {
        for (n, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            // Un documento es prosa entera, así que no hace falta separar
            // cadenas de comentarios como en `src/`: cualquier párrafo en
            // español lleva algún acento.
            if line.chars().any(|c| "áéíóúñÁÉÍÓÚÑ¿¡".contains(c)) {
                offenders.push(format!("{}:{} {}", f.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} líneas en español:\n{}",
        offenders.len(),
        offenders.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn every_documented_mem_command_exists() {
    let help = String::from_utf8(
        std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
            .arg("--help").output().unwrap().stdout,
    ).unwrap();
    // Los subcomandos que clap lista, uno por línea indentada.
    let known: Vec<String> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    assert!(!known.is_empty(), "no pude leer los subcomandos de la ayuda:\n{help}");

    let mut offenders = Vec::new();
    for f in living_docs() {
        let text = std::fs::read_to_string(&f).unwrap();
        let mut in_block = false;
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_block = line.trim_start().starts_with("```bash") || line.trim_start().starts_with("```sh");
                continue;
            }
            if !in_block { continue; }
            let t = line.trim_start().trim_start_matches("$ ");
            let Some(resto) = t.strip_prefix("mem ") else { continue };
            // Saltea las banderas globales para llegar al subcomando.
            let sub = resto
                .split_whitespace()
                .find(|w| !w.starts_with('-') && !w.contains('/') && !w.contains('~'));
            let Some(sub) = sub else { continue };
            if !known.iter().any(|k| k == sub) {
                offenders.push(format!("{}:{} `mem {sub}` no existe", f.display(), n + 1));
            }
        }
    }
    assert!(offenders.is_empty(), "comandos documentados que no existen:\n{}", offenders.join("\n"));
}
