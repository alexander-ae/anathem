//! What happens when whatever was reading `mem`'s output walks away.
//!
//! `mem search x | head -1`, `mem recent | head`, a `less` the user quits
//! early: all of them close the pipe while the command is still writing.
//! That is ordinary use of a command line tool, and the convention is to die
//! quietly from `SIGPIPE`, the way `grep` and `cat` do. Rust ignores the
//! signal before `main` runs, so without `die_on_closed_pipe` the write
//! returns `EPIPE` and `println!` panics on it, printing a backtrace note at
//! a user who did nothing wrong.
//!
//! Unix only: there is no `SIGPIPE` on Windows, and nothing to restore.

#![cfg(unix)]

use std::os::unix::io::FromRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// A stdout nobody will ever read: a pipe whose read end is closed *before*
/// the child exists, so its very first write fails.
///
/// Closing the read end after spawning would leave a race in the test — a
/// short message fits in the pipe buffer and succeeds — and what is being
/// checked here is how the failed write is handled, not who wins that race.
fn stdout_nobody_reads() -> Stdio {
    let mut fds = [0i32; 2];
    // SAFETY: `fds` is a two-element array, which is what `pipe` writes.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe() failed");
    // SAFETY: the read end was just opened by `pipe` and is closed once.
    assert_eq!(unsafe { libc::close(fds[0]) }, 0, "close() failed");
    // SAFETY: the write end is live and ownership moves into the `Stdio`.
    unsafe { Stdio::from_raw_fd(fds[1]) }
}

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn a_closed_pipe_kills_the_process_instead_of_panicking() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "a note about indexing and search"]).status.success());

    let child = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(&db)
        .args(["search", "indexing"])
        .stdout(stdout_nobody_reads())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !stderr.contains("panicked"),
        "a closed pipe should not panic, but stderr says:\n{stderr}"
    );
    // The positive half: it did not merely avoid the panic, it ended the way
    // every other tool in a pipeline ends.
    assert_eq!(
        out.status.signal(),
        Some(libc::SIGPIPE),
        "expected death by SIGPIPE, got {:?} with stderr:\n{stderr}",
        out.status
    );
}
