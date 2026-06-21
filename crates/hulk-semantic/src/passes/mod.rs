//! Semantic analysis passes.
//!
//! This module contains the four passes of the semantic analyzer:
//! - `collect`: declaration collection (Pass 0)
//! - `hierarchy`: inheritance and protocol resolution (Pass 1)
//! - `infer`: type inference (Pass 2)
//! - `check`: type checking (Pass 3)

mod collect;
mod hierarchy;
mod infer;
mod check;

pub use collect::run as collect;
pub use hierarchy::run as hierarchy;
pub use infer::run as infer;
pub use check::run as check;