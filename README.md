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
│ hulk-codegen │  Generates executable output (not yet implemented)
└──────────────┘
```

Each stage is its own crate and only depends on the ones before it:

| Crate | Status | Depends on |
|---|---|---|
| `hulk-ast` | Done — shared AST types used by every later stage | — |
| `hulk-lexer` | Done | `hulk-ast` |
| `hulk-parser` | Done | `hulk-ast`, `hulk-lexer` |
| `hulk-semantic` | Done | `hulk-ast` |
| `hulk-codegen` | **Not yet implemented** (empty crate, scaffolded for future work) | `hulk-ast`, `hulk-semantic` |
| `hulk-cli` | Wires lexer → parser → semantic analysis together | `hulk-lexer`, `hulk-parser`, `hulk-semantic` |

## Requirements

- [Rust](https://rustup.rs/) 1.94.1 or later

## Setup
```bash
git clone https://github.com/D4R102004/hulk-compiler.git
cd hulk-compiler
cargo build --all
```

## Usage
```bash
cargo run -p hulk-cli -- path/to/program.hulk
# lexes, parses, and runs full semantic analysis on the program.
# - On success: prints any warnings, then the fully type-annotated AST
#   (every expression now carries its resolved type).
# - On failure: prints every semantic error found (not just the first one),
#   each with its source location, and exits with a non-zero status.
```

There is no code generation yet, so this does not produce an executable — it
validates that the program is lexically, syntactically, and semantically
correct, and shows the typed AST that a future `hulk-codegen` stage would
consume.

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

## Running Tests
```bash
cargo test --all
```

This runs the lexer, parser, and semantic-analysis test suites, including
`hulk-semantic`'s integration tests, which lex, parse, and `analyze` real
HULK source end to end.

## Team

| Name | GitHub |
|------|--------|
| Darío Francisco Alfonso Urrutia | [@D4R102004](https://github.com/D4R102004) |
| Juan Carlos Carmenate Díaz | [@Juank404](https://github.com/JuanCMath) |
| Sebastian González Alfonso | [@sebagonz106](https://github.com/sebagonz106) |
