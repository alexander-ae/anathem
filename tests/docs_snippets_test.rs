//! The MCP configuration is written out twice, and the two copies have to
//! agree.
//!
//! `AGENTS.md` opens by saying that an agent asked to install anathem needs
//! nothing else from the repository, so it carries its own copy of the
//! `mcpServers` entry and of opencode's. That duplication is deliberate and
//! worth keeping — a brief that sends the reader somewhere else is not a
//! brief. What is not acceptable is the pair drifting: these are snippets
//! somebody pastes verbatim into a file an application reads, so a stale
//! argument here is an installation that fails with no explanation.
//!
//! Nothing in the suite noticed the two copies before this test, which is the
//! only reason to keep a duplication at all: that somebody checks it.

use std::path::Path;

/// The home directory stands in for a real one differently in each
/// document — `AGENTS.md` writes `/home/USUARIO`, `installation.md` writes
/// `/path/to/your/home` — because they address different readers. Only the
/// placeholder may differ.
fn normalize(s: &str) -> String {
    s.replace("/home/USUARIO", "$HOME")
        .replace("/path/to/your/home", "$HOME")
        .replace("/Users/USUARIO", "$HOME")
}

/// Every `"anathem": { ... }` object in a document, read by balancing braces
/// rather than by parsing: both files hold fragments, and one of them is
/// JSONC.
fn anathem_entries(file: &str) -> Vec<serde_json::Value> {
    let text = normalize(&std::fs::read_to_string(Path::new(file)).unwrap());
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = text[from..].find("\"anathem\"") {
        let rest = &text[from + at..];
        let Some(open) = rest.find('{') else { break };
        let mut depth = 0i32;
        let mut end = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(stop) = end else { break };
        let body = &rest[open..stop];
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            out.push(v);
        }
        from += at + stop;
    }
    assert!(!out.is_empty(), "{file} has no «anathem» snippet");
    out
}

/// The entry a given agent takes: opencode's carries `type`, Claude Code's
/// carries `args`. Picking by shape rather than by position keeps the test
/// from caring which snippet a document prints first.
fn entry_for(file: &str, agent: &str) -> serde_json::Value {
    let key = if agent == "opencode" { "type" } else { "args" };
    let mut found: Vec<_> =
        anathem_entries(file).into_iter().filter(|v| v.get(key).is_some()).collect();
    assert_eq!(
        found.len(),
        1,
        "{file} trae {} snippets de {agent}; tiene que traer exactamente uno",
        found.len()
    );
    found.remove(0)
}

#[test]
fn both_documents_give_claude_code_the_same_entry() {
    let agents = entry_for("AGENTS.md", "claude-code");
    let install = entry_for("docs/installation.md", "claude-code");
    assert_eq!(
        agents, install,
        "AGENTS.md y docs/installation.md dan configuraciones distintas a Claude Code"
    );
    // And it is the invocation the binary actually accepts, not just two
    // copies of the same mistake.
    assert_eq!(agents["args"][0], "--db");
    assert_eq!(agents["args"][2], "serve");
    assert!(agents["command"].as_str().unwrap().ends_with("/mem"));
}

#[test]
fn both_documents_give_opencode_the_same_entry() {
    let agents = entry_for("AGENTS.md", "opencode");
    let install = entry_for("docs/installation.md", "opencode");
    assert_eq!(
        agents, install,
        "AGENTS.md y docs/installation.md dan configuraciones distintas a opencode"
    );
    // opencode takes the binary and its arguments as one array, which is the
    // difference that makes two snippets necessary in the first place.
    let cmd = agents["command"].as_array().unwrap();
    assert!(cmd[0].as_str().unwrap().ends_with("/mem"));
    assert_eq!(cmd[1], "--db");
    assert_eq!(cmd[3], "serve");
    assert_eq!(agents["enabled"], true);
}

/// The absolute path is the whole point of these two snippets: a desktop
/// application is started by the system, not by a shell, so it inherits
/// neither `PATH` nor anything that expands `~`.
#[test]
fn neither_snippet_uses_a_tilde_or_a_variable() {
    for (file, marker) in [
        ("AGENTS.md", "claude-code"),
        ("AGENTS.md", "opencode"),
        ("docs/installation.md", "claude-code"),
        ("docs/installation.md", "opencode"),
    ] {
        let raw = serde_json::to_string(&entry_for(file, marker)).unwrap();
        // `$HOME` here is this test's own placeholder, put in by `normalize`.
        let real = raw.replace("$HOME", "");
        assert!(!real.contains('~'), "{file} ({marker}) uses ~ in a path: {raw}");
        assert!(
            !real.contains("$HOME") && !real.contains("${"),
            "{file} ({marker}) usa una variable en una ruta: {raw}"
        );
    }
}
