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
┌─────────────┐
│ hulk-lexer  │  Tokenizes source code into a stream of tokens
└──────┬──────┘
       │
    ▼
┌─────────────┐
│ hulk-parser │  Builds an Abstract Syntax Tree (AST)
└──────┬──────┘
       │
       ▼
┌──────────────────┐
│ hulk-semantic    │  Type checking and semantic validation
└──────┬───────────┘
       │
       ▼
┌──────────────┐
│ hulk-codegen │  Generates executable output
└──────────────┘
```

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
```

## Running Tests
```bash
cargo test --all
```

## Team

| Name | GitHub |
|------|--------|
| Darío Alfonso | [@D4R102004](https://github.com/D4R102004) |
| Juan Carlos Carmenate| [@Juank404](https://github.com/JuanCMath) |
| Sebastián González Alfonso| [@sebagonz106](https://github.com/sebagonz106) |

## License

MIT