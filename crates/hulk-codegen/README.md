# hulk-codegen

Code generation for HULK: lowers a fully type-checked `hulk_semantic::VerifiedProgram`
into a Linux x86_64 executable via LLVM, links it against the `hulk-rt` runtime library,
and produces a standalone executable.

## Toolchain and Cross-Compilation

**Development is on Windows; all generated executables are Linux x86_64 binaries.** This is
a cross-compiler by design. The compiler itself (written in Rust) can be compiled and
developed on any platform (Windows, macOS, Linux), but every HULK program it produces
runs exclusively on Linux x86_64, regardless of the development machine.

This crate is pinned to **LLVM 17** (`inkwell` feature `llvm17-0`), matching the LLVM
17.0.6 toolchain used on the team's Windows development machines.

### On Windows (primary development environment)

```powershell
./scripts/setup_llvm17_windows.ps1
# follow the printed instruction to set $env:LLVM_SYS_170_PREFIX, then:
cargo build -p hulk-rt --release
cargo run -p hulk-codegen --example smoke --release
```

This produces a valid Linux x86_64 binary at the printed path. Copy it to your WSL/Ubuntu
instance and run it there to confirm execution works. The smoke test validates the
build chain but defers execution to the target platform, since Windows cannot run ELF
binaries directly.

### On WSL/Ubuntu (when compiling from there, secondary)

```bash
./scripts/setup_llvm17_ubuntu.sh
# follow the printed instruction, then:
cargo build -p hulk-rt --release
cargo run -p hulk-codegen --example smoke --release
```

On Linux/WSL, the smoke binary is still targeting Linux x86_64 (same as Windows), so
the final execution step works directly and prints `SUCCESS`.

### Toolchain requirements

- **LLVM 17** (development headers)
- **`clang`** or **`gcc`** with Linux x86_64 cross-compilation support. On Windows, this
  can be part of the official LLVM 17 distribution or installed via `vcpkg`. On Linux/WSL,
  standard Ubuntu packages work.
