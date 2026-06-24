//! HULK runtime support library.
//!
//! This crate is built once as a static library (`libhulk_rt.a`) and linked
//! into every executable produced by `hulk-codegen`. Every public function is
//! `extern "C"` with a stable `#[no_mangle]` name so generated LLVM IR can
//! call it by symbol name without depending on Rust's own calling
//! conventions or name mangling.

#[no_mangle]
pub extern "C" fn hulk_rt_noop() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_does_not_panic() {
        hulk_rt_noop();
    }
}
