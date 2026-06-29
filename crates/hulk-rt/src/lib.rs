//! HULK runtime support library.
//!
//! This crate is built once as a static library (`libhulk_rt.a`) and linked
//! into every executable produced by `hulk-codegen`. Every public function is
//! `extern "C"` with a stable `#[no_mangle]` name so generated LLVM IR can
//! call it by symbol name without depending on Rust's own calling
//! conventions or name mangling.

use std::alloc::{alloc, dealloc, Layout};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── Type tags ─────────────────────────────────────────────────────────
pub const TAG_STRING: u8 = 0;
pub const TAG_VECTOR: u8 = 1;
pub const TAG_BOX: u8   = 2;
pub const TAG_RANGE: u8 = 3;
pub const TAG_NUMBER: u8 = 4;         // used inside HulkBox
pub const TAG_BOOLEAN: u8 = 5;        // used inside HulkBox
pub const TAG_DYN_VEC: u8 = 6;        // used for dynamic vectors (comprehensions)
pub const TAG_LITERAL_STRING: u8 = 7; // used for string literals (immutable, immortal)
pub const TAG_OBJECT: u8 = 8;         // used for object instances

// ─── Object header ─────────────────────────────────────────────────────
#[repr(C)]
pub struct ObjHeader {
    pub ref_count: i64,
    pub gc_mark: u8,          // bool in C, stored as u8
    pub type_tag: u8,         // 0 = String, 1 = Vector, 2 = Box
    pub next: *mut ObjHeader,
    pub vtable: *const (),
}

// ─── HulkString ────────────────────────────────────────────────────────
#[repr(C)]
pub struct HulkString {
    pub header: ObjHeader,
    pub len: i64,
    pub data: *mut u8,
}

// ─── HulkVector ────────────────────────────────────────────────────────
#[repr(C)]
pub struct HulkVector {
    pub header: ObjHeader,
    pub len: i64,
    pub current_index: i64, // For iteration
    pub data: *mut *mut std::ffi::c_void,
}

// ─── Dynamic vector (for comprehensions) ──────────────────────────────
#[repr(C)]
pub struct HulkDynamicVector {
    pub header: ObjHeader,
    pub data: Vec<*mut std::ffi::c_void>,
}

// ─── HulkBox ───────────────────────────────────────────────────────────
#[repr(C)]
pub struct HulkBox {
    pub header: ObjHeader,
    pub original_tag: u8,    // TAG_NUMBER, TAG_BOOLEAN
    pub _padding: [u8; 7],
    pub payload: i64,        // bitcast of f64 or bool (as i64)
}

// ─── HulkRange ─────────────────────────────────────────────────────────
#[repr(C)]
pub struct HulkRange {
    pub header: ObjHeader,
    pub min: f64,
    pub max: f64,
    pub current: f64,
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Allocates a HulkString from a byte slice.
unsafe fn hulk_rt_string_from_bytes(data: &[u8]) -> *mut HulkString {
    let len = data.len() as i64;
    let string_layout = Layout::new::<HulkString>();
    let string_ptr = alloc(string_layout) as *mut HulkString;
    if string_ptr.is_null() {
        return ptr::null_mut();
    }

    let data_layout = Layout::array::<u8>(len as usize).unwrap();
    let data_ptr = alloc(data_layout);
    if data_ptr.is_null() {
        dealloc(string_ptr as *mut u8, string_layout);
        return ptr::null_mut();
    }
    ptr::copy(data.as_ptr(), data_ptr, len as usize);

    ptr::write(string_ptr, HulkString {
        header: ObjHeader {
            ref_count: 1,
            gc_mark: 0,
            type_tag: TAG_STRING,
            next: ptr::null_mut(),
            vtable: ptr::null(),
        },
        len,
        data: data_ptr,
    });
    string_ptr
}

fn is_immortal_ptr(ptr: *mut std::ffi::c_void) -> bool {
    if ptr.is_null() { return false; }
    unsafe {
        let header = ptr as *mut ObjHeader;
        (*header).type_tag == TAG_LITERAL_STRING
    }
}

fn is_immortal_header(header: *mut ObjHeader) -> bool {
    if header.is_null() { return false; }
    unsafe {
        (*header).type_tag == TAG_LITERAL_STRING
    }
} 

// ─── Base runtime functions ──────────────────────────────────────────────

/// A no-op function that does nothing.
///
/// Used as a placeholder to test that the runtime is correctly linked.
#[no_mangle]
pub extern "C" fn hulk_rt_noop() {}

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
pub extern "C" fn hulk_rt_string_concat(
    a: *mut std::ffi::c_void,
    b: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    if a.is_null() || b.is_null() { return ptr::null_mut(); }
    unsafe {
        let sa = a as *mut HulkString;
        let sb = b as *mut HulkString;
        let len_a = (*sa).len as usize;
        let len_b = (*sb).len as usize;
        let total_len = len_a + len_b;
        let mut buf = Vec::with_capacity(total_len);
        buf.extend_from_slice(std::slice::from_raw_parts((*sa).data, len_a));
        buf.extend_from_slice(std::slice::from_raw_parts((*sb).data, len_b));
        hulk_rt_string_from_bytes(&buf) as *mut std::ffi::c_void
    }
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
pub extern "C" fn hulk_rt_string_concat_space(
    a: *mut std::ffi::c_void,
    b: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    if a.is_null() || b.is_null() { return ptr::null_mut(); }
    unsafe {
        let sa = a as *mut HulkString;
        let sb = b as *mut HulkString;
        let len_a = (*sa).len as usize;
        let len_b = (*sb).len as usize;
        let total_len = len_a + 1 + len_b;
        let mut buf = Vec::with_capacity(total_len);
        buf.extend_from_slice(std::slice::from_raw_parts((*sa).data, len_a));
        buf.push(b' ');
        buf.extend_from_slice(std::slice::from_raw_parts((*sb).data, len_b));
        hulk_rt_string_from_bytes(&buf) as *mut std::ffi::c_void
    }
}

/// Returns true if two HULK strings have identical byte content.
///
/// # Safety
/// Both pointers must point to valid `HulkString` objects.
#[no_mangle]
pub extern "C" fn hulk_rt_string_equals(
    a: *mut std::ffi::c_void,
    b: *mut std::ffi::c_void,
) -> bool {
    if a.is_null() && b.is_null() { return true; }
    if a.is_null() || b.is_null() { return false; }
    unsafe {
        let sa = a as *mut HulkString;
        let sb = b as *mut HulkString;
        let len_a = (*sa).len as usize;
        let len_b = (*sb).len as usize;
        if len_a != len_b { return false; }
        let slice_a = std::slice::from_raw_parts((*sa).data, len_a);
        let slice_b = std::slice::from_raw_parts((*sb).data, len_b);
        slice_a == slice_b
    }
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
pub extern "C" fn hulk_rt_number_to_string(num: f64) -> *mut std::ffi::c_void {
    let s = num.to_string();
    unsafe { hulk_rt_string_from_bytes(s.as_bytes()) as *mut std::ffi::c_void }
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
pub extern "C" fn hulk_rt_bool_to_string(b: bool) -> *mut std::ffi::c_void {
    let s = if b { "true" } else { "false" };
    unsafe { hulk_rt_string_from_bytes(s.as_bytes()) as *mut std::ffi::c_void }
}

/// Prints a HULK object to standard output.
#[no_mangle]
pub extern "C" fn hulk_rt_print(obj: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    use std::io::Write;
    if obj.is_null() {
        println!("null");
        return obj;
    }
    unsafe {
        let header = obj as *mut ObjHeader;
        match (*header).type_tag {
            TAG_STRING | TAG_LITERAL_STRING => {
                let s = obj as *mut HulkString;
                let len = (*s).len as usize;
                let data = std::slice::from_raw_parts((*s).data, len);
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(data);
                let _ = out.write_all(b"\n");
            }
            TAG_BOX => {
                let boxed = obj as *mut HulkBox;
                match (*boxed).original_tag {
                    TAG_NUMBER => {
                        let val = f64::from_bits((*boxed).payload as u64);
                        println!("{}", val);
                    }
                    TAG_BOOLEAN => {
                        let val = (*boxed).payload != 0;
                        println!("{}", val);
                    }
                    _ => println!("<unknown box>"),
                }
            }
            TAG_VECTOR => {
                let vec = obj as *mut HulkVector;
                let len = (*vec).len;
                print!("[");
                for i in 0..len {
                    let elem = hulk_rt_vector_get(vec, i);
                    if i > 0 { print!(", "); }
                    if elem.is_null() { print!("null"); }
                    else {
                        // For simplicity, print the address; can be improved later.
                        print!("<obj@{:p}>", elem);
                    }
                }
                println!("]");
            }
            _ => println!("<object>"),
        }
    }
    obj
}

// ─── Memory management ─────────────────────────────────────────────────

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
pub extern "C" fn hulk_rt_alloc(size: i64) -> *mut std::ffi::c_void {
    if size <= 0 { return ptr::null_mut(); }
    let layout = Layout::from_size_align(size as usize, 8).unwrap_or_else(|_| Layout::new::<u8>());
    unsafe { alloc(layout) as *mut std::ffi::c_void }
}

/// Increments the reference count of an object pointed to by `ptr`.
/// 
/// Useful for managing the lifetime of objects in a reference-counted memory model.
#[no_mangle]
pub extern "C" fn hulk_rt_retain(ptr: *mut std::ffi::c_void) {
    if
        ptr.is_null() ||
        is_immortal_ptr(ptr) // Retain is a no-op for immortal types.
    { return; }
    unsafe {
        let header = ptr as *mut ObjHeader;
        // WHY: ref_count == -1 is the immortal sentinel (CPython PEP 683 pattern).
        // String literals live in read-only .rodata; attempting to write would SIGSEGV.
        if (*header).ref_count == -1 { return; }
        (*header).ref_count += 1;
    }
}

/// Decrements the reference count of an object pointed to by `ptr`.
/// If the reference count reaches zero, the object is deallocated.
#[no_mangle]
pub extern "C" fn hulk_rt_release(ptr: *mut std::ffi::c_void) {
    if
        ptr.is_null() ||
        is_immortal_ptr(ptr) // Retain is a no-op for immortal types.
    { return; }
    unsafe {
        let header = ptr as *mut ObjHeader;
        // WHY: ref_count == -1 is the immortal sentinel (CPython PEP 683 pattern).
        // String literals live in read-only .rodata; attempting to write would SIGSEGV.
        if (*header).ref_count == -1 { return; }
        (*header).ref_count -= 1;
        if (*header).ref_count == 0 {
            match (*header).type_tag {
                TAG_VECTOR => {
                    let vec = ptr as *mut HulkVector;
                    let data = (*vec).data;
                    let len = (*vec).len as usize;
                    let data_layout = Layout::array::<*mut std::ffi::c_void>(len).unwrap();
                    dealloc(data as *mut u8, data_layout);
                    let vec_layout = Layout::new::<HulkVector>();
                    dealloc(vec as *mut u8, vec_layout);
                }
                TAG_STRING => {
                    let s = ptr as *mut HulkString;
                    let data = (*s).data;
                    let len = (*s).len as usize;
                    let data_layout = Layout::array::<u8>(len).unwrap();
                    dealloc(data, data_layout);
                    let string_layout = Layout::new::<HulkString>();
                    dealloc(s as *mut u8, string_layout);
                }
                TAG_BOX => {
                    let boxed = ptr as *mut HulkBox;
                    let box_layout = Layout::new::<HulkBox>();
                    dealloc(boxed as *mut u8, box_layout);
                }
                TAG_RANGE => {
                    let range = ptr as *mut HulkRange;
                    let range_layout = Layout::new::<HulkRange>();
                    dealloc(range as *mut u8, range_layout);
                }
                TAG_DYN_VEC => {
                    // Reconstruct the Box and drop it, which frees the Vec and the struct.
                    let _ = Box::from_raw(ptr as *mut HulkDynamicVector);
                }
                _ => {
                    // Fallback for unknown tags (should not happen)
                    if is_immortal_header(header) { return; } // Do not deallocate immortal objects
                    let layout = Layout::from_size_align(32, 8).unwrap();
                    dealloc(ptr as *mut u8, layout);
                }
            }
        }
    }
}

// ─── Vector Functions ──────────────────────────────────────────────────

/// Allocates a HulkVector and its data array, sets initial refcount to 1, and returns the pointer.
#[no_mangle]
pub extern "C" fn hulk_rt_vector_new(len: i64) -> *mut HulkVector {
    if len < 0 { return ptr::null_mut(); }

    // 1. Allocate the HulkVector struct itself.
    let vec_layout = Layout::new::<HulkVector>();
    let vec_ptr = unsafe { alloc(vec_layout) as *mut HulkVector };
    if vec_ptr.is_null() { return ptr::null_mut(); }

    // 2. Allocate the data array (len pointers).
    let data_layout = Layout::array::<*mut std::ffi::c_void>(len as usize)
        .unwrap_or_else(|_| Layout::new::<*mut std::ffi::c_void>());
    let data_ptr = unsafe { alloc(data_layout) as *mut *mut std::ffi::c_void };
    if data_ptr.is_null() {
        unsafe { dealloc(vec_ptr as *mut u8, vec_layout); }
        return ptr::null_mut();
    }

    // 3. Zero-initialise the data array.
    unsafe {
        ptr::write_bytes(data_ptr, 0, len as usize);
    }

    // 4. Fill the vector fields.
    unsafe {
        ptr::write(vec_ptr, HulkVector {
            header: ObjHeader {
                ref_count: 1,
                gc_mark: 0,
                type_tag: TAG_VECTOR,
                next: ptr::null_mut(),
                vtable: ptr::null(),
            },
            len,
            current_index: -1, // To call next() before current() is mandatory.
            data: data_ptr,
        });
    }

    vec_ptr
}

/// Returns the number of elements in a HulkVector.
///
/// # Safety
/// `vec` must be a valid, aligned pointer to a live `HulkVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_vector_size(vec: *mut HulkVector) -> i64 {
    if vec.is_null() { return 0; }
    (*vec).len
}

/// Retrieves the element at the given index from a HulkVector.
///
/// Note: This function does not perform bounds checking; the caller must ensure that `index` is valid.
///
/// # Safety
/// `vec` must be null or a valid, aligned pointer to a live `HulkVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_vector_get(vec: *mut HulkVector, index: i64) -> *mut std::ffi::c_void {
    if vec.is_null() || index < 0 { return ptr::null_mut(); }
    unsafe {
        let data = (*vec).data;
        let len = (*vec).len;
        if index >= len { return ptr::null_mut(); }
        *data.offset(index as isize)
    }
}

/// Sets the element at the given index in a HulkVector to a new value, managing reference counts appropriately.
///
/// # Safety
/// `vec` must be null or a valid, aligned pointer to a live `HulkVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_vector_set(
    vec: *mut HulkVector,
    index: i64,
    value: *mut std::ffi::c_void,
) {
    if vec.is_null() || index < 0 { return; }
    unsafe {
        let data = (*vec).data;
        let len = (*vec).len;
        if index >= len { return; }

        let slot = data.offset(index as isize);
        let old = *slot;

        // Release the old value (if any)
        if !old.is_null() {
            hulk_rt_release(old);
        }

        // Store the new value and retain it (if non‑null)
        if !value.is_null() {
            hulk_rt_retain(value);
        }
        *slot = value;
    }
}

/// Advances the current index of the vector for iteration.
///
/// Returns true if there is a next element, false otherwise.
///
/// # Safety
/// `vec` must be null or a valid, aligned pointer to a live `HulkVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_vector_next(vec: *mut HulkVector) -> bool {
    if vec.is_null() { return false; }
    unsafe {
        let idx = (*vec).current_index + 1; // Vectors are 0-indexed
        if idx < (*vec).len {
            (*vec).current_index = idx;
            true
        } else {
            false
        }
    }
}

/// Returns the current element in the vector based on the current index.
///
/// Note: The caller must ensure that `hulk_rt_vector_next` has been called
/// and returned true before calling this function.
///
/// # Safety
/// `vec` must be null or a valid, aligned pointer to a live `HulkVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_vector_current(vec: *mut HulkVector) -> *mut std::ffi::c_void {
    if vec.is_null() { return ptr::null_mut(); }
    unsafe {
        let idx = (*vec).current_index;
        let pos = idx;
        if pos < 0 || pos >= (*vec).len { return ptr::null_mut(); }
        let data = (*vec).data;
        *data.offset(pos as isize)
    }
}

// ─── Dynamic vector helpers for comprehensions ────────────────────────

#[no_mangle]
pub extern "C" fn hulk_rt_dynamic_vector_new() -> *mut HulkDynamicVector {
    let vec = HulkDynamicVector {
        header: ObjHeader {
            ref_count: 1,
            gc_mark: 0,
            type_tag: TAG_DYN_VEC,
            next: ptr::null_mut(),
            vtable: ptr::null(),
        },
        data: Vec::new(),
    };
    Box::into_raw(Box::new(vec))
}

/// Appends a value to a dynamic vector, retaining it.
///
/// # Safety
/// `vec` must be null or a valid, aligned pointer to a live `HulkDynamicVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_dynamic_vector_append(
    vec: *mut HulkDynamicVector,
    value: *mut std::ffi::c_void,
) {
    if vec.is_null() { return; }
    unsafe {
        let vec_ref = &mut *vec;
        vec_ref.data.push(value);
        if !value.is_null() {
            hulk_rt_retain(value);
        }
    }
}

/// Converts a dynamic vector into a fixed-size `HulkVector`, consuming the dynamic vector.
///
/// # Safety
/// `dyn_vec` must be null or a valid, aligned pointer to a live `HulkDynamicVector`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_dynamic_vector_to_vector(
    dyn_vec: *mut HulkDynamicVector,
) -> *mut HulkVector {
    if dyn_vec.is_null() { return ptr::null_mut(); }
    unsafe {
        let vec_ref = &mut *dyn_vec;
        let len = vec_ref.data.len() as i64;
        let fixed = hulk_rt_vector_new(len);
        if fixed.is_null() {
            // Release the dynamic vector (drops the Vec and the struct)
            drop(Box::from_raw(dyn_vec));
            return ptr::null_mut();
        }
        for (i, &val) in vec_ref.data.iter().enumerate() {
            hulk_rt_vector_set(fixed, i as i64, val);
        }
        // Release the dynamic vector (drops the Vec and the struct)
        drop(Box::from_raw(dyn_vec));
        fixed
    }
}

// ─── Range Functions ──────────────────────────────────────────────────

/// Creates a new HulkRange object with the specified minimum and maximum values.
#[no_mangle]
pub extern "C" fn hulk_rt_range_new(min: f64, max: f64) -> *mut HulkRange {
    let layout = Layout::new::<HulkRange>();
    let ptr = unsafe { alloc(layout) as *mut HulkRange };
    if ptr.is_null() { return ptr::null_mut(); }
    unsafe {
        ptr::write(ptr, HulkRange {
            header: ObjHeader {
                ref_count: 1,
                gc_mark: 0,
                type_tag: TAG_RANGE,
                next: ptr::null_mut(),
                vtable: ptr::null(),
            },
            min,
            max,
            current: min - 1.0,
        });
    }
    ptr
}

/// Advances the current value of the range for iteration.
///
/// Returns true if there is a next value, false otherwise.
///
/// # Safety
/// `rng` must be null or a valid, aligned pointer to a live `HulkRange`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_range_next(rng: *mut HulkRange) -> bool {
    if rng.is_null() { return false; }
    unsafe {
        (*rng).current += 1.0;
        (*rng).current < (*rng).max
    }
}

/// Returns the current value of the range.
///
/// Note: The caller must ensure that `hulk_rt_range_next` has been called
/// and returned true before calling this function.
///
/// # Safety
/// `rng` must be null or a valid, aligned pointer to a live `HulkRange`.
#[no_mangle]
pub unsafe extern "C" fn hulk_rt_range_current(rng: *mut HulkRange) -> f64 {
    if rng.is_null() { return 0.0; }
    unsafe { (*rng).current }
}

// ─── Math builtin functions ──────────────────────────────────────────────

/// Returns the square root of a floating-point number.
#[no_mangle]
pub extern "C" fn hulk_rt_sqrt(x: f64) -> f64 { x.sqrt() }

/// Returns the sine of a floating-point number (in radians).
#[no_mangle]
pub extern "C" fn hulk_rt_sin(x: f64) -> f64 { x.sin() }

/// Returns the cosine of a floating-point number (in radians).
#[no_mangle]
pub extern "C" fn hulk_rt_cos(x: f64) -> f64 { x.cos() }

/// Returns the exponential of a floating-point number (e^x).
#[no_mangle]
pub extern "C" fn hulk_rt_exp(x: f64) -> f64 { x.exp() }

/// Returns the logarithm of a floating-point number with the specified base.
#[no_mangle]
pub extern "C" fn hulk_rt_log(base: f64, x: f64) -> f64 { x.log(base) }

// ─── Random Number Generator ─────────────────────────────────────────────────

/// A simple thread-safe pseudo-random number generator (PRNG) using an atomic state.
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Initializes the RNG state with a seed based on the current system time.
fn init_rng() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    RNG_STATE.store(seed ^ 0x9e3779b97f4a7c15, Ordering::Relaxed);
}

/// Returns the next 64-bit random number.
fn next_u64() -> u64 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    if x == 0 { init_rng(); x = RNG_STATE.load(Ordering::Relaxed); }
    x ^= x << 7;
    x ^= x >> 9;
    RNG_STATE.store(x, Ordering::Relaxed);
    x
}

/// Returns a random floating-point number in the range [0, 1).
#[no_mangle]
pub extern "C" fn hulk_rt_rand() -> f64 {
    let bits = next_u64() >> 11;
    (bits as f64) / (1u64 << 53) as f64
}

// ─── Downcast and match traps ──────────────────────────────────────────

/// Checks if the object can be downcast to the target vtable type.
#[no_mangle]
pub extern "C" fn hulk_rt_downcast_check(
    obj: *mut std::ffi::c_void,
    target_vtable: *const (),
) -> bool {
    if obj.is_null() || target_vtable.is_null() {
        return false;
    }
    unsafe {
        let header = obj as *mut ObjHeader;
        let mut vtable = (*header).vtable;
        while !vtable.is_null() {
            if vtable == target_vtable {
                return true;
            }
            // Read the parent pointer (first element of the vtable array)
            let parent_ptr = vtable as *const *const ();
            vtable = *parent_ptr;
        }
        false
    }
}

/// Called when a downcast fails; prints an error message and aborts the program.
#[no_mangle]
pub extern "C" fn hulk_rt_downcast_fail() -> ! {
    eprintln!("runtime error: downcast failed");
    std::process::abort();
}

/// Called when a non-exhaustive match is encountered; prints an error message and aborts the program.
#[no_mangle]
pub extern "C" fn hulk_rt_match_fail() -> ! {
    eprintln!("runtime error: non-exhaustive match");
    std::process::abort();
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;
    
    /// Helper: Converts a pointer to a `HulkString` into a Rust `String`.
    unsafe fn string_from_ptr(ptr: *mut std::ffi::c_void) -> String {
        if ptr.is_null() { return String::new(); }
        let s = ptr as *mut HulkString;
        let len = (*s).len as usize;
        let data = std::slice::from_raw_parts((*s).data, len);
        String::from_utf8_lossy(data).to_string()
    }

    /// Captures stdout while executing the given closure and returns the output as a String.
    fn capture_stdout<F>(f: F) -> String
    where
        F: FnOnce(),
    {
        unsafe {
            let mut pipe_fds = [0; 2];
            assert_eq!(libc::pipe(pipe_fds.as_mut_ptr()), 0, "pipe creation failed");

            let stdout_fd = libc::dup(1);
            assert!(stdout_fd >= 0, "dup failed");

            // Redirect stdout to the write end of the pipe.
            assert_eq!(libc::dup2(pipe_fds[1], 1), 1, "dup2 failed");

            // Close the write end in the parent (the child's writes go through the duplicate fd)
            libc::close(pipe_fds[1]);

            // Execute the closure.
            f();

            // Ensure the output is flushed.
            let _ = std::io::stdout().flush();
            libc::fflush(std::ptr::null_mut());

            // Restore stdout.
            libc::dup2(stdout_fd, 1);
            libc::close(stdout_fd);

            // Read from the read end of the pipe.
            let mut buffer = Vec::new();
            let mut file = std::fs::File::from_raw_fd(pipe_fds[0]);
            file.read_to_end(&mut buffer).expect("reading from pipe failed");

            String::from_utf8_lossy(&buffer).to_string()
        }
    }

    /// Smoke test that verifies that calling hulk_rt_noop does not cause a panic or crash.
    #[test]
    fn noop_does_not_panic() {
        hulk_rt_noop();
    }

    /// Smoke test that verifies that calling hulk_rt_retain and hulk_rt_release 
    /// on a valid pointer does not cause an immediate runtime failure.
    /// Minimal sanity check for the memory management infrastructure.
    #[test]
    fn retain_release_does_not_crash() {
        let ptr = hulk_rt_alloc(64);
        assert!(!ptr.is_null());
        hulk_rt_retain(ptr);
        hulk_rt_release(ptr);
        hulk_rt_release(ptr); // should deallocate
    }

    // ─── String tests ────────────────────────────────────────────────────────────────

    /// Tests that converting a number to a string produces the expected result.
    #[test]
    fn number_to_string() {
        let ptr = hulk_rt_number_to_string(42.0);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!(string_from_ptr(ptr), "42");
            hulk_rt_release(ptr);
        }
    }

    /// Tests that converting a boolean to a string produces the expected result.
    #[test]
    fn bool_to_string() {
        let ptr = hulk_rt_bool_to_string(true);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!(string_from_ptr(ptr), "true");
            hulk_rt_release(ptr);
        }
        let ptr2 = hulk_rt_bool_to_string(false);
        assert!(!ptr2.is_null());
        unsafe {
            assert_eq!(string_from_ptr(ptr2), "false");
            hulk_rt_release(ptr2);
        }
    }

    /// Tests that concatenating two strings produces the expected result.
    #[test]
    fn string_concat() {
        let a = hulk_rt_number_to_string(10.0);
        let b = hulk_rt_bool_to_string(true);
        let c = hulk_rt_string_concat(a, b);
        assert!(!c.is_null());
        unsafe {
            assert_eq!(string_from_ptr(c), "10true");
            hulk_rt_release(a);
            hulk_rt_release(b);
            hulk_rt_release(c);
        }
    }

    /// Tests that concatenating two strings with a space produces the expected result.
    #[test]
    fn string_concat_space() {
        let a = hulk_rt_number_to_string(10.0);
        let b = hulk_rt_bool_to_string(true);
        let c = hulk_rt_string_concat_space(a, b);
        assert!(!c.is_null());
        unsafe {
            assert_eq!(string_from_ptr(c), "10 true");
            hulk_rt_release(a);
            hulk_rt_release(b);
            hulk_rt_release(c);
        }
    }
    
    // ─── Vector tests ────────────────────────────────────────────────────────────────

    /// Tests that creating a vector, setting values, and retrieving them works as expected.
    #[test]
    fn vector_new_and_get_set() {
        let vec = hulk_rt_vector_new(3);
        assert!(!vec.is_null());
        unsafe {
            // Set values
            let v1 = hulk_rt_number_to_string(1.0);
            let v2 = hulk_rt_number_to_string(2.0);
            let v3 = hulk_rt_number_to_string(3.0);
            hulk_rt_vector_set(vec, 0, v1);
            hulk_rt_vector_set(vec, 1, v2);
            hulk_rt_vector_set(vec, 2, v3);
            // Get and verify
            let r1 = hulk_rt_vector_get(vec, 0);
            let r2 = hulk_rt_vector_get(vec, 1);
            let r3 = hulk_rt_vector_get(vec, 2);
            assert_eq!(string_from_ptr(r1), "1");
            assert_eq!(string_from_ptr(r2), "2");
            assert_eq!(string_from_ptr(r3), "3");
            // Release all
            hulk_rt_release(v1);
            hulk_rt_release(v2);
            hulk_rt_release(v3);
            hulk_rt_release(vec as *mut std::ffi::c_void);
        }
    }

    /// Tests that iterating over a vector works as expected.
    #[test]
    fn vector_iterator() {
        let vec = hulk_rt_vector_new(3);
        assert!(!vec.is_null());
        unsafe {
            // Fill
            let v1 = hulk_rt_number_to_string(1.0);
            let v2 = hulk_rt_number_to_string(2.0);
            let v3 = hulk_rt_number_to_string(3.0);
            hulk_rt_vector_set(vec, 0, v1);
            hulk_rt_vector_set(vec, 1, v2);
            hulk_rt_vector_set(vec, 2, v3);
            // Iterate
            let mut count = 0;
            let mut results = Vec::new();
            while hulk_rt_vector_next(vec) {
                let cur = hulk_rt_vector_current(vec);
                results.push(string_from_ptr(cur));
                count += 1;
            }
            assert_eq!(count, 3);
            assert_eq!(results, vec!["1", "2", "3"]);
            hulk_rt_release(v1);
            hulk_rt_release(v2);
            hulk_rt_release(v3);
            hulk_rt_release(vec as *mut std::ffi::c_void);
        }
    }
    
    // ─── Range tests ────────────────────────────────────────────────────────────────

    /// Tests that creating a range and iterating over its values works as expected.
    #[test]
    fn range_basic() {
        let rng = hulk_rt_range_new(1.0, 5.0);
        assert!(!rng.is_null());
        let mut values = Vec::new();
        // SAFETY: rng is non-null (checked above) and points to a valid HulkRange.
        unsafe {
            while hulk_rt_range_next(rng) {
                values.push(hulk_rt_range_current(rng));
            }
        }
        assert_eq!(values, vec![1.0, 2.0, 3.0, 4.0]);
        hulk_rt_release(rng as *mut std::ffi::c_void);
    }

    /// Tests that creating an empty range works as expected.
    #[test]
    fn range_empty() {
        let rng = hulk_rt_range_new(5.0, 5.0);
        assert!(!rng.is_null());
        // SAFETY: rng is non-null (checked above) and points to a valid HulkRange.
        assert!(unsafe { !hulk_rt_range_next(rng) });
        hulk_rt_release(rng as *mut std::ffi::c_void);  
    }

    // ─── Math tests ────────────────────────────────────────────────────────────

    /// Tests that the math functions work as expected.
    #[test]
    fn math_functions() {
        assert_eq!(hulk_rt_sqrt(4.0), 2.0);
        assert!((hulk_rt_sin(0.0) - 0.0).abs() < 1e-10);
        assert!((hulk_rt_cos(0.0) - 1.0).abs() < 1e-10);
        assert!((hulk_rt_exp(1.0) - std::f64::consts::E).abs() < 1e-10);
        assert!((hulk_rt_log(2.0, 8.0) - 3.0).abs() < 1e-10);
    }

    /// Tests that the random number generator produces values in the expected range.
    #[test]
    fn rand_returns_in_range() {
        let r = hulk_rt_rand();
        assert!((0.0..1.0).contains(&r));
    }

    // ─── Print tests ──────────────────────────────────────────────────

    /// Tests that printing a number outputs the expected string representation.
    #[test]
    fn print_outputs_number() {
        let s = hulk_rt_number_to_string(42.0);
        let output = capture_stdout(|| {
            let _ = hulk_rt_print(s);
        });
        assert_eq!(output, "42\n");
        hulk_rt_release(s);
    }

    /// Tests that printing a boolean outputs the expected string representation.
    #[test]
    fn print_outputs_boolean() {
        let s = hulk_rt_bool_to_string(true);
        let output = capture_stdout(|| {
            let _ = hulk_rt_print(s);
        });
        assert_eq!(output, "true\n");
        hulk_rt_release(s);
    }

    /// Tests that printing a string outputs the expected string representation.
    #[test]
    fn print_outputs_string_directly() {
        let s = hulk_rt_number_to_string(123.45);
        let output = capture_stdout(|| {
            let _ = hulk_rt_print(s);
        });
        // Note: the formatting may vary; we accept exact representation.
        assert!(output == "123.45\n", "output was: {:?}", output);
        hulk_rt_release(s);
    }

    /// Tests that printing outputs the expected string representation. 
    #[test]
    fn print_returns_its_argument() {
        let s = hulk_rt_number_to_string(99.0);
        let result = hulk_rt_print(s);
        assert_eq!(result, s);
        hulk_rt_release(s);
    }

    /// Tests that printing handles null pointers.
    #[test]
    fn print_handles_null() {
        let result = hulk_rt_print(ptr::null_mut());
        assert!(result.is_null());
    }

    /// Tests dynamic vector append and conversion to a fixed-size vector.
    #[test]
    fn dynamic_vector_append_and_to_vector() {
        let dyn_vec = hulk_rt_dynamic_vector_new();
        assert!(!dyn_vec.is_null());
        unsafe {
            let v1 = hulk_rt_number_to_string(1.0);
            let v2 = hulk_rt_number_to_string(2.0);
            let v3 = hulk_rt_number_to_string(3.0);
            hulk_rt_dynamic_vector_append(dyn_vec, v1);
            hulk_rt_dynamic_vector_append(dyn_vec, v2);
            hulk_rt_dynamic_vector_append(dyn_vec, v3);
            let fixed = hulk_rt_dynamic_vector_to_vector(dyn_vec);
            assert!(!fixed.is_null());
            // Check fixed vector length.
            assert_eq!((*fixed).len, 3);
            // Check elements.
            let e1 = hulk_rt_vector_get(fixed, 0);
            let e2 = hulk_rt_vector_get(fixed, 1);
            let e3 = hulk_rt_vector_get(fixed, 2);
            assert_eq!(string_from_ptr(e1), "1");
            assert_eq!(string_from_ptr(e2), "2");
            assert_eq!(string_from_ptr(e3), "3");
            // Clean up.
            hulk_rt_release(fixed as *mut std::ffi::c_void);
            hulk_rt_release(v1);
            hulk_rt_release(v2);
            hulk_rt_release(v3);
        }
    }

}
