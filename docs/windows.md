# anathem on Windows

The release publishes an `x86_64-pc-windows-msvc` binary, and every push
compiles with MSVC on a `windows-latest` runner and runs the full suite plus
a pass through `init`, `save`, `search` and `doctor`. One test is skipped
here, the POSIX permissions one, for the reason its name gives; the one that
reads `/proc` runs only on Linux. The counts are deliberately not written
down — a number that changes on every commit is a line of documentation that
is wrong more often than it is right.

What CI cannot cover is anything that depends on which shell you start from,
because it runs everything under one. Those checks, and one question about
file mappings that cannot be answered from Linux at all, are in
[issue #57](https://gitlab.com/__alexander__/anathem/-/issues/57). The rest
of this document is what is settled.

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

## Known minor debt

`replace_file` leaves a sidecar file — `{name}.anathem-pack.reemplazando-{pid}`
— if the process dies between moving the target aside and finishing. It is
inert: nothing resolving a pack path or checking a pack's identity looks at
it, so it confuses neither. `mem doctor` does look, on purpose, and lists any
it finds as leftovers from an interrupted install; they can be deleted by
hand with no consequences.
