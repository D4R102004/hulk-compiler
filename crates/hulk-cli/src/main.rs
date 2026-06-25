//! Entry point for the HULK compiler.
//!
//! Reads a `.hulk` source file, runs it through the compiler pipeline,
//! and reports any errors with precise source locations.

use std::fs;
use std::process;

use clap::Parser as clapParser;
use hulk_lexer::Lexer;
use hulk_parser::Parser;
use hulk_semantic::analyze;

/// The HULK compiler.
#[derive(clapParser)]
#[command(version, about = "HULK language compiler", long_about = None)]
struct Args {
    /// Path to the .hulk source file to compile.
    file: String,
}

fn main() {
    // Pipeline: read source → lex (exit 1) → parse (exit 2) → semantic analysis (exit 3).
    // Exit codes are mandated by the matcom/compilers grader interface contract.

    // Parse command line arguments.
    let args = Args::parse();

    // Read the source file.
    let source = fs::read_to_string(&args.file).unwrap_or_else(|err| {
        eprintln!("error: could not read '{}': {}", args.file, err);
        // Pre-pipeline I/O failure; exit 1 is the closest applicable code.
        process::exit(1);
    });

    // Lex the source code.
    let tokens = Lexer::new(&source).tokenize().unwrap_or_else(|err| {
        eprintln!("error: {:?}", err);
        // Grader contract: exit 1 = lexical error.
        process::exit(1);
    });

    // Parse the token stream into an AST.
    let program = Parser::new(tokens).parse_program().unwrap_or_else(|err| {
        eprintln!("error: {}", err);
        // Grader contract: exit 2 = syntactic error.
        process::exit(2);
    });

    match analyze(&program) {
        Err(errors) => {
            // Print every semantic error (not just the first).
            for error in &errors {
                eprintln!("{}", error);
            }
            // Grader contract: exit 3 = semantic error.
            process::exit(3);
        }
        Ok(verified) => {
            // Print warnings, if any.
            for warning in &verified.warnings {
                eprintln!("warning: {}", warning);
            }

            // Print the fully typed AST (each expression now carries a Type).
            println!("{:#?}", verified.typed_program);
        }
    }
}
