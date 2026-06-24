//! HULK runtime support library.
//!
//! This crate is built once as a static library (`libhulk_rt.a`) and linked
//! into every executable produced by `hulk-codegen`. Every public function is
//! `extern "C"` with a stable `#[no_mangle]` name so generated LLVM IR can
//! call it by symbol name without depending on Rust's own calling
//! conventions or name mangling.

// ─── Base runtime functions ──────────────────────────────────────────────

/// A no-op function that does nothing.
///
/// Used as a placeholder to test that the runtime is correctly linked.
#[no_mangle]
pub extern "C" fn hulk_rt_noop() {}

/// Allocates a block of memory of the given size in bytes.
///
/// # Parameters
/// - `size`: The number of bytes to allocate. Must be greater than zero.
///
/// # Returns
/// A pointer to the newly allocated memory block, or a null pointer if
/// allocation fails. The allocated memory is zero-initialised (guaranteed by
/// `std::alloc`).
///
/// # Safety
/// This function is safe to call from any context, but the caller is
/// responsible for freeing the allocated memory via the corresponding
/// deallocation function.
#[no_mangle]
pub extern "C" fn hulk_rt_alloc(_size: i64) -> *mut std::ffi::c_void {
    unimplemented!("hulk_rt_alloc is not yet implemented in Phase 3")
}

/// Concatenates two HULK strings without adding a separator.
///
/// # Parameters
/// - `a`: Pointer to the first `HulkString` object.
/// - `b`: Pointer to the second `HulkString` object.
///
/// # Returns
/// A pointer to a newly allocated `HulkString` object that contains the
/// concatenation of `a` followed by `b`.
///
/// # Safety
/// The caller must ensure that both pointers point to valid `HulkString`
/// objects, and that the strings are immutable (no mutation during the call).
#[no_mangle]
pub extern "C" fn hulk_rt_string_concat(_a: *mut std::ffi::c_void, _b: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    unimplemented!("hulk_rt_string_concat is not yet implemented in Phase 3")
}

/// Concatenates two HULK strings with a single space inserted between them.
///
/// # Parameters
/// - `a`: Pointer to the first `HulkString` object.
/// - `b`: Pointer to the second `HulkString` object.
///
/// # Returns
/// A pointer to a newly allocated `HulkString` object that contains the
/// concatenation of `a`, a literal space, and `b`.
///
/// # Safety
/// The caller must ensure that both pointers point to valid `HulkString`
/// objects.
#[no_mangle]
pub extern "C" fn hulk_rt_string_concat_space(_a: *mut std::ffi::c_void, _b: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    unimplemented!("hulk_rt_string_concat_space is not yet implemented in Phase 3")
}

/// Converts a 64-bit floating-point number to its string representation.
///
/// # Parameters
/// - `num`: The number to convert.
///
/// # Returns
/// A pointer to a newly allocated `HulkString` object containing the
/// decimal representation of `num`.
#[no_mangle]
pub extern "C" fn hulk_rt_number_to_string(_num: f64) -> *mut std::ffi::c_void {
    unimplemented!("hulk_rt_number_to_string is not yet implemented in Phase 3")
}

/// Converts a boolean value to its string representation.
///
/// # Parameters
/// - `b`: The boolean value to convert (0 = false, 1 = true).
///
/// # Returns
/// A pointer to a newly allocated `HulkString` object containing `"true"` or
/// `"false"`.
#[no_mangle]
pub extern "C" fn hulk_rt_bool_to_string(_b: bool) -> *mut std::ffi::c_void {
    unimplemented!("hulk_rt_bool_to_string is not yet implemented in Phase 3")
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_does_not_panic() {
        hulk_rt_noop();
    }

    // TODO: Add tests for the other runtime functions once they are implemented.
}
