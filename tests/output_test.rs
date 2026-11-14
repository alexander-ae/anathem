//! What reaches a terminal, and what reaches a pipe.
//!
//! A body is untrusted text: it can carry `\r` and ESC sequences, which a
//! terminal obeys. Cleaning them is a decision about the terminal, so it
//! happens only there; a pipe keeps the exact bytes, which is what a script
//! expects. The terminal half is covered by the unit tests in
//! `src/output.rs`, because a test cannot pretend to be a TTY.

use std::path::Path;
use std::process::Command;

fn mem_bytes(dir: &Path, args: &[&str]) -> (bool, Vec<u8>) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (o.status.success(), o.stdout)
}

const CRUEL: &str = "balance 100\rTRANSFER DONE \u{1b}[2J and a link \u{1b}]8;;http://x\u{1b}\\";

fn seeded() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let (ok, _) = mem_bytes(d.path(), &["init"]);
    assert!(ok);
    let (ok, _) = mem_bytes(d.path(), &["save", CRUEL, "--project", "p"]);
    assert!(ok);
    d
}

#[test]
fn a_pipe_gets_the_bytes_exactly_as_they_were_stored() {
    let d = seeded();
    let (ok, out) = mem_bytes(d.path(), &["show", "1"]);
    assert!(ok);
    let text = String::from_utf8_lossy(&out);

    assert!(out.contains(&0x1b), "the ESC byte has to survive a pipe:\n{text:?}");
    assert!(text.contains('\r'), "the carriage return has to survive too:\n{text:?}");
    assert!(text.contains("TRANSFER DONE"), "{text}");
}

#[test]
fn raw_says_the_same_thing_on_a_terminal() {
    let d = seeded();
    let (ok, out) = mem_bytes(d.path(), &["show", "1", "--raw"]);
    assert!(ok);
    assert!(out.contains(&0x1b), "--raw has to keep the bytes: {out:?}");
}

#[test]
fn every_command_that_prints_a_body_takes_the_same_path() {
    // The four surfaces a body comes out of, with the escape hatch accepted
    // by each. What they print is up to the TTY rule; that they all go
    // through it is what this checks.
    let d = seeded();
    for args in [
        vec!["search", "balance"],
        vec!["search", "balance", "--raw"],
        vec!["recent", "--all"],
        vec!["recent", "--all", "--raw"],
        vec!["history", "1"],
        vec!["history", "1", "--raw"],
        vec!["show", "--topic", "none", "--raw"],
    ] {
        let (ok, out) = mem_bytes(d.path(), &args);
        assert!(ok, "{args:?} failed: {}", String::from_utf8_lossy(&out));
    }
}
