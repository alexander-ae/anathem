# anathem on Windows

**Status: it compiles and passes the suite in CI; it still has to be tried by
hand.** Every push compiles with MSVC on a `windows-latest` runner and runs
249 tests (one stays ignored: the POSIX permissions one, which does not apply
on Windows), plus a run through `init`, `save`, `search` and `doctor`. Before
that, nobody had ever compiled it on Windows.

What CI **cannot** cover is further down, under "What to test": CI runs
everything under a single shell, so it does not compare the default path
between `cmd.exe` and Git Bash, which is the point where a misplaced
precedence would leave two different databases without saying anything.

For reference: Linux passes 251 and macOS 250 (the missing one reads `/proc`,
which only exists on Linux).

## What had to be fixed

The production code never used `std::os::unix` APIs. The four blockers were
about operating system semantics:

| | problem | fix |
|---|---|---|
| 1 | `HOME` does not exist on Windows, and the default path `~/.anathem/mem.db` failed | `USERPROFILE` is used, taking precedence over `HOME` only on Windows |
| 2 | `std::fs::rename` **fails if the target exists** on Windows, whereas on POSIX it replaces it | a `replace_file` helper that moves the target aside, renames, and only then deletes what it moved aside |
| 3 | Windows does not allow renaming a file with an open memory mapping | `reconcile` drops the `Pack` before renaming and reopens it afterwards |
| 4 | a test used `PermissionsExt`, so the suite did not compile | `#[cfg(unix)]` plus a companion `#[cfg(windows)] #[ignore]` test so the count does not lie |

About point 1 there is a detail that is not obvious: in Git Bash and MSYS,
`HOME` does exist, but in posix format (`/c/Users/someone`). For a **native**
Windows binary that is not a valid user path — a leading `/` with no drive
letter resolves against the root of the current drive. If `HOME` took
precedence, the same person running the same binary from `cmd.exe` and from
Git Bash would end up with **two different databases** with no warning at all.
That is why `USERPROFILE` wins on Windows.

The fixes are **platform neutral** on purpose: they use `cfg!(windows)` — a
macro that evaluates to a boolean and lets both branches compile — instead of
`#[cfg(windows)]`, which would leave code uncompiled and untested on Linux.
The boolean is passed as a parameter, so the tests exercise **both**
precedences and both default editors from Linux. The only platform `#[cfg]`
is in the test for point 4, where it is structurally unavoidable.

## Compiling

A C toolchain is needed, just as on Linux: the project compiles SQLite
(`rusqlite` with `bundled`) and oniguruma (`tokenizers`) from source.

    rustup toolchain install stable-x86_64-pc-windows-msvc
    cargo build --release
    cargo test

With MSVC you need the *Build Tools for Visual Studio* with the C++ workload.
If `cargo build` fails in `libsqlite3-sys` or in `onig_sys`, that is what it
is, not anathem's code.

## What to test, in this order

1. ~~**That it compiles.**~~ Done: CI compiles with MSVC on every push. It was
   the only thing that could not be verified in any way from Linux.
2. **The default path.** Run `mem init` from `cmd.exe` and then from Git Bash,
   and check that both print **the same** path. If they differ, the variable
   precedence is wrong and you are going to end up with two databases.
3. **`mem model install` over an already installed pack.** This is the case
   that failed on Windows with the original `rename`. It has to reinstall
   without an error and leave the pack file intact and complete.
4. **The unresolved critical scenario, below.**
5. **`mem edit` with no `$EDITOR` defined.** It should open `notepad`.

## What is NOT resolved and has to be measured first

**Reinstalling the pack while the MCP server is running.** The server lives
for hours with the pack file mapped into memory. If `mem model install` of the
same pack is run from another terminal, the installing process drops *its*
`Pack`, but it cannot drop the server's. On Windows, if the other process's
mapping blocks the rename, the installation would fail — and that is a use the
design claims to support, not an edge case.

It depends on which sharing flags `memmap2` opens the file with, something
that cannot be determined without running it. **It is the first experiment to
run**: start `mem serve`, and from another terminal reinstall the same pack.
If it fails, the likely fix is for the server to drop and reopen the mapping
when it detects the change, which is what `reconcile` already does within a
single process.

## Known minor debt

`replace_file` leaves a sidecar file — `{name}.anathem-pack.reemplazando-{pid}`
— if the process dies between moving the target aside and finishing. It is
inert garbage: it was verified that no production function enumerates
`paquetes/` by pattern, so it confuses neither path resolution nor the pack's
identity guard. It can be deleted by hand with no consequences.
