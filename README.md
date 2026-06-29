# HULK Compiler

![CI](https://github.com/D4R102004/hulk-compiler/actions/workflows/ci.yml/badge.svg)

> A compiler for the [HULK language](https://matcom.github.io/hulk/appendix-hulk-syntax.html),
> implemented entirely in Rust.
> Universidad de La Habana — Compilers 2026.

## What is HULK?

HULK (Havana University Language for Kompilers) is a statically-typed,
object-oriented, expression-based language designed at UH for teaching compilers.

## Architecture
```
Source Code
    │
    ▼
┌──────────────┐
│ hulk-lexer   │  Tokenizes source code into a stream of tokens
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ hulk-parser  │  Builds an Abstract Syntax Tree (AST) with a hand-written LL(1) predictive parser
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ hulk-semantic│  Name resolution, inheritance/protocol resolution, type inference, and type checking
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ hulk-codegen │  Lowers the typed AST to LLVM IR and emits Linux x86_64 object files
└──────┬───────┘
       │
       ▼
   Executable
```

Each stage is its own crate and only depends on the ones before it:

| Crate | Status | Depends on |
|---|---|---|
| `hulk-ast` | Done — shared AST types used by every later stage | — |
| `hulk-lexer` | Done | `hulk-ast` |
| `hulk-parser` | Done | `hulk-ast`, `hulk-lexer` |
| `hulk-semantic` | Done | `hulk-ast` |
| `hulk-codegen` | Done — lowers to LLVM, emits Linux x86_64 object code, and links against the `hulk-rt` runtime; currently exposed via the smoke example and internal tests, not yet integrated into the CLI driver | `hulk-ast`, `hulk-semantic` |
| `hulk-cli` | Wires lexer → parser → semantic analysis together | `hulk-lexer`, `hulk-parser`, `hulk-semantic` |

## Requirements

- [Rust](https://rustup.rs/) 1.94.1 or later
- LLVM 17 development headers and tools (for code generation). See the `hulk-codegen` README for platform-specific setup.

## Setup
```bash
git clone https://github.com/D4R102004/hulk-compiler.git
cd hulk-compiler
cargo build --all
```

## Usage

### Semantic validation (CLI)
```bash
cargo run -p hulk-cli -- path/to/program.hulk
# lexes, parses, and runs full semantic analysis on the program.
# - On success: prints any warnings, then the fully type-annotated AST
#   (every expression now carries its resolved type).
# - On failure: prints every semantic error found (not just the first one),
#   each with its source location, and exits with a non-zero status.
```

### Code generation (smoke test)
```bash
# Build the runtime library first
cargo build -p hulk-rt --release
# Run the smoke test, which compiles a minimal program to a Linux x86_64 executable
cargo run -p hulk-codegen --example smoke --release
# The resulting executable is placed in a temporary directory and can be copied
# to a Linux environment (WSL/Ubuntu) for execution.
```

The compiler currently produces Linux x86_64 binaries, even when developed on Windows. The `smoke` example demonstrates the full end-to-end pipeline from IR to object file to linked executable. Full integration of code generation into the CLI driver is planned for the next phase.

## Current Frontend Status

- `hulk-lexer` converts source code into `Vec<Token>`.
- `hulk-parser` consumes those tokens and produces a `hulk_ast::Program` (the
  untyped AST).
- The parser is implemented manually as an LL(1) predictive recursive-descent parser. Every grammar decision uses one token of lookahead; expression precedence is encoded by a left-recursion-free grammar with tail productions implemented as loops. The parser crate also includes `GRAMMAR_LL1.md`, which documents the grammar shape and the mapping from grammar non-terminals to Rust methods.

Supported parser coverage includes global functions, types, protocols, `let`, blocks, `if`/`elif`/`else`, `while`, `for`, function/method calls, member access, object construction, destructive assignment, vectors, indexing, `is`/`as`, and the planned `match` extension node.

## Current Semantic Analysis Status

`hulk-semantic` takes the untyped AST produced by `hulk-parser` and turns it
into a fully type-checked, fully type-*annotated* AST, ready to be handed to
a code generator. It owns name resolution, inheritance and protocol
resolution, type inference, and final type checking.

The public entry point is:

```rust
pub fn analyze(program: &hulk_ast::Program) -> Result<VerifiedProgram, Vec<SemanticError>>
```

`VerifiedProgram` is the contract for everything downstream (codegen):

```rust
pub struct VerifiedProgram {
    pub registry: TypeRegistry,        // every resolved type/protocol/function signature
    pub typed_program: TypedProgram,    // Program<Type> — every expression carries a resolved Type
    pub warnings: Vec<SemanticError>,   // non-fatal diagnostics
}
```

`analyze` never stops at the first problem: every failure path collects a
`SemanticError` (with severity `Error` or `Warning`) instead of bailing out,
so a single call surfaces every issue in the program at once.

Internally, `analyze` runs five passes over a shared `TypeRegistry`:

| # | Pass | Responsibility |
|---|------|-----------------|
| 0 | Collect | Registers every function, type, and protocol signature, so forward references work regardless of declaration order. |
| 1 | Hierarchy | Resolves `inherits`/`extends` links, rejects invalid inheritance, detects cycles, checks override and protocol-variance rules, and flattens attribute/method tables. |
| 1.5 | Resolve constructor params | Infers unannotated type-constructor parameters and propagates them through `inherits Parent(args)` clauses. |
| 2 | Infer | Builds the fully typed tree, assigning a type to every expression and resolving every unannotated symbol it can. |
| 3 | Check | Re-validates explicit annotations, sweeps for any leftover unresolved types, and enforces attribute privacy. |

Type system coverage includes: the builtin value types `Number`/`String`/
`Boolean`; a single nominal root `Object`; user-defined types and protocols
(single inheritance for types, structural conformance for protocols, with
contravariant parameters / covariant return types); `Vector<T>` and
`Iterable<T>`; and lowest-common-ancestor resolution for multi-branch
constructs (`if`/`elif`/`else`, vector literals, `match`).

**Known limitations** (documented, not accidental gaps):

- No functors or lambda expressions yet — there is no `Lambda` AST node or
  `(T) -> R` arrow-type syntax.
- Mutual recursion between two functions that *both* lack an explicit return
  type annotation is not resolved (each function's placeholder type can't be
  observed by the other while both are mid-inference); this case currently
  produces a deterministic "cannot infer type" error.

## Current Code Generation Status

`hulk-codegen` lowers a fully type-checked `VerifiedProgram` into a Linux x86_64
executable via LLVM. It supports:

- **Object layout** – builds struct types, computes field offsets, and creates
  vtables for virtual dispatch.
- **Itables** – generates interface tables for protocol dispatch.
- **Lowering** – translates all major HULK constructs to LLVM IR, including:
  - Literals, variables, `let`, assignments, blocks.
  - Unary/binary operators, arithmetic, comparisons, concatenation.
  - Control flow (`if`/`elif`/`else`, `while`, `for`).
  - Function and method calls (static, virtual, and `base` delegation).
  - Object construction (`new`), member access, attribute privacy.
  - Type tests (`is`) and downcasts (`as`), with runtime checks.
  - Vector literals and comprehensions.
  - Pattern matching (`match`) with exhaustive checking.
- **Runtime integration** – declares and links against `hulk-rt`, a small
  runtime library that provides allocation, string operations, and iteration
  support.
- **Cross‑compilation** – produces Linux ELF binaries even when developed on
  Windows, leveraging LLVM's target machine and `clang`/`cc` as the linker.

The codegen pipeline is exercised by the `smoke` example and by integration tests
inside the crate. Full integration into the CLI driver is planned for the next
phase.

## Running Tests
```bash
cargo test --all
```

This runs the lexer, parser, semantic-analysis, and code-generation test suites,
including end-to-end tests that compile real HULK source to object files.

## Team

| Name | GitHub |
|------|--------|
| Darío Francisco Alfonso Urrutia | [@D4R102004](https://github.com/D4R102004) |
| Juan Carlos Carmenate Díaz | [@Juank404](https://github.com/JuanCMath) |
| Sebastian González Alfonso | [@sebagonz106](https://github.com/sebagonz106) |
