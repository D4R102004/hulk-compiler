//! Entry point for the HULK compiler.
//!
//! Reads a `.hulk` source file, runs it through the compiler pipeline,
//! and reports any errors with precise source locations.

use std::fs;
use std::process;

use clap::Parser;
use hulk_lexer::Lexer;
use hulk_parser::Parser;

/// The HULK compiler.
#[derive(Parser)]
#[command(version, about = "HULK language compiler", long_about = None)]
struct Args {
    /// Path to the .hulk source file to compile.
    file: String,
}

fn main() {
    // Parse command line arguments.
    // WHY: clap automatically handles --help, --version,
    // and missing argument errors — no manual checking needed.
    let args = Args::parse();

    // Read the source file into a string.
    // WHY: we report the filename in the error so the user
    // knows exactly which file failed to open.
    let source = fs::read_to_string(&args.file).unwrap_or_else(|err| {
        eprintln!("error: could not read '{}': {}", args.file, err);
        process::exit(1);
    });

    // Lex the source code.
    let tokens = Lexer::new(&source).tokenize().unwrap_or_else(|err| {
        eprintln!("error: {:?}", err);
        process::exit(1);
    });

    // Parse the token stream into an AST.
    // WHY: the parser is now the second real frontend phase, so the CLI
    // prints the AST instead of stopping at lexical debugging.
    let program = Parser::new(tokens).parse_program().unwrap_or_else(|err| {
        eprintln!("error: {}", err);
        process::exit(1);
    });

    println!("{:#?}", program);
}
