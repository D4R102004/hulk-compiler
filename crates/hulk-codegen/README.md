# hulk-codegen

Code generation for HULK: lowers a fully type-checked `hulk_semantic::VerifiedProgram`
into a Linux x86_64 executable via LLVM, links it against the `hulk-rt` runtime library,
and produces a standalone executable.

## Overview

`hulk-codegen` is the final stage of the HULK compiler pipeline. It consumes the
typed AST and type registry produced by semantic analysis and emits native machine
code. The pipeline is:

1. **Build object layouts** – compute LLVM struct types for every user-defined
   class, including the object header (ref count, GC mark, next pointer, vtable),
   inherited fields, and attribute offsets.

2. **Declare all functions and methods** – create LLVM function declarations for
   every global function and method, storing them in the codegen context for later
   use.

3. **Build vtables** – for each type, create a global array of function pointers
   for its methods (flattened with inheritance). Used for virtual dispatch.

4. **Build itables** – for every protocol that is actually used, and for every
   type that structurally implements it, emit an interface table (itable) containing
   function pointers for the protocol's methods in declaration order.

5. **Define functions and methods** – lower each function/method body to LLVM IR,
   including parameter binding, local variables, and the full expression tree.

6. **Lower the entry expression** – compile the program's top-level expression,
   executing it for side effects.

7. **Verify the module** – run LLVM's `Module::verify()` to catch internal
   lowering bugs.

8. **Emit object code** – write a relocatable ELF object file (`.o`) for
   `x86_64-unknown-linux-gnu`.

9. **Link against the runtime** – invoke the system linker (`clang` or `cc`) to
   produce a final Linux executable.

The crate is designed for cross‑compilation: it can be built and run on any
platform (Windows, macOS, Linux) but always generates Linux x86_64 binaries.

## Supported Language Features

The code generator supports the entire core HULK language and most extensions:

- **Literals**: numbers, booleans, strings (as global constants)
- **Variables and scoping**: `let` bindings, shadowing, lexical scopes
- **Arithmetic**: `+`, `-`, `*`, `/`, `%`, `^` (pow) on `Number`
- **Comparisons**: `==`, `!=`, `<`, `<=`, `>`, `>=`
- **Logical operators**: `&`, `|`, `!`
- **String concatenation**: `@` and `@@` (with automatic stringification)
- **Control flow**: `if`/`elif`/`else`, `while`, `for` loops (over `Iterable`)
- **Blocks**: sequence of expressions returning the last value
- **Assignments**: `:=` for variables and member fields
- **Functions and methods**: declarations, calls, recursion, `base` delegation
- **Object construction**: `new Type(args)` with constructor parameters
- **Member access**: attributes and methods (method references become fat pointers)
- **Type tests and downcasts**: `is` and `as` with runtime checks
- **Vectors**: literals, indexing, and comprehensions (`[expr | var in iterable]`)
- **Pattern matching**: `match` with literals, type patterns, and wildcards (exhaustiveness checked at runtime)
- **Protocols**: structural conformance, itable-based dispatch

## Architecture

The code generation is organised into several modules:

| Module | Responsibility |
|--------|----------------|
| `context` | Shared mutable state: LLVM context, module, builder, symbol tables |
| `layout` | Object layout, field offsets, vtable construction |
| `itables` | Interface table generation for protocol dispatch |
| `lower` | Expression lowering to LLVM IR, split into submodules per construct |
| `emit` | Target machine initialisation and object file emission |
| `runtime_decls` | Declarations of `hulk-rt` functions as external LLVM symbols |
| `options` | Compilation options (output path, optimisation level) |
| `error` | Code generation error types |

The lowering is performed by a recursive `lower_expr` function that matches on
the AST node and delegates to specialised submodules:

- `literal` – constants
- `binding` – variables, `let`, assignment
- `operators` – unary and binary operators
- `control` – blocks, `if`, `while`
- `call` – function and method calls (static, virtual, base)
- `member` – attribute access and method references
- `new` – object construction
- `type_ops` – `is` and `as`
- `for_loop` – `for` loops and vector comprehensions
- `pattern` – `match` expressions
- `decl` / `method` – function and method declarations/definitions

## Current Status

- **Implemented** – full lowering of all language constructs listed above.
- **Tested** – extensive unit and integration tests (see `src/lib.rs` tests)
  that compile real HULK source strings to valid ELF object files.
- **Smoke test** – the `smoke` example builds a minimal program, emits IR,
  produces an object file, links it against `hulk-rt`, and validates the ELF
  magic bytes.
- **Integration** – code generation is not yet exposed through the main CLI
  (`hulk-cli`). The `compile` function is available as a public API for
  programmatic use, and integration into the driver is planned for the next phase.

### On Windows

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

### On WSL/Ubuntu

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

## Usage (Programmatic)

The crate exposes a `compile` function that takes a `VerifiedProgram` and
`CodegenOptions` and produces an executable:

```rust
use hulk_codegen::{compile, CodegenOptions};

let verified: hulk_semantic::VerifiedProgram = /* ... */;
let opts = CodegenOptions::with_output_path("output");
compile(&verified, &opts)?;
```

This will create `output.o` and then link it (future: automatically produce the
final executable). Currently, linking is not fully automated; the smoke example
shows how to invoke the linker manually. The `CodegenOptions` struct also supports
setting an optimisation level and optionally emitting the LLVM IR for debugging.

## Testing

Run the test suite with:

```bash
cargo test -p hulk-codegen
```

This includes:
- Unit tests for lowering individual expressions (via IR string inspection).
- Integration tests that lex, parse, analyse, and compile real source snippets,
  checking that the resulting object file is a valid ELF binary.
- The smoke example, which exercises the full build chain.