/// Tagged pointer value representation for Thoxa
/// 
/// All values in Thoxa are 64-bit tagged pointers:
/// - 0bXXXXXXX1 = small integer (63-bit signed)
/// - 0bXXXXX000 = pointer to heap object (8-byte aligned)
/// - 0bXXXXX010 = false
/// - 0bXXXXX110 = true
/// - 0bXXXX0100 = nil/null
/// - 0bXXXX1100 = undefined (reserved)
/// - 0bXXXXX100 = (reserved for future special values)

use crate::runtime::BuiltinTypeId;

pub type Value = u64;

// Tag masks and values
const TAG_MASK: u64 = 0x7;
const TAG_INT: u64 = 0x1;
const TAG_PTR: u64 = 0x0;
const TAG_FALSE: u64 = 0x2;
const TAG_TRUE: u64 = 0x6;
const TAG_NIL: u64 = 0x4;

// Special singletons
pub const FALSE_VALUE: Value = TAG_FALSE;
pub const TRUE_VALUE: Value = TAG_TRUE;
pub const NIL_VALUE: Value = TAG_NIL;

/// Object header stored at the beginning of every heap object
#[repr(C)]
pub struct ObjectHeader {
    pub type_id: u32,
    pub size: u32,
}

/// Tagged integer creation and extraction
#[inline]
pub fn tag_int(n: i64) -> Value {
    ((n << 3) as u64) | TAG_INT
}

#[inline]
pub fn untag_int(v: Value) -> Option<i64> {
    if is_int(v) {
        Some((v as i64) >> 3)
    } else {
        None
    }
}

/// Boolean creation
#[inline]
pub fn tag_bool(b: bool) -> Value {
    if b { TRUE_VALUE } else { FALSE_VALUE }
}

#[inline]
pub fn untag_bool(v: Value) -> Option<bool> {
    match v {
        FALSE_VALUE => Some(false),
        TRUE_VALUE => Some(true),
        _ => None,
    }
}

/// Pointer tagging (assumes 8-byte alignment)
#[inline]
pub fn tag_ptr(ptr: *const u8) -> Value {
    debug_assert!(ptr as u64 & TAG_MASK == 0, "Pointer must be 8-byte aligned");
    ptr as u64
}

#[inline]
pub fn untag_ptr(v: Value) -> Option<*const u8> {
    if is_ptr(v) {
        Some((v & !TAG_MASK) as *const u8)
    } else {
        None
    }
}

/// Type checking
#[inline]
pub fn is_int(v: Value) -> bool {
    (v & TAG_MASK) == TAG_INT
}

#[inline]
pub fn is_ptr(v: Value) -> bool {
    (v & TAG_MASK) == TAG_PTR
}

#[inline]
pub fn is_bool(v: Value) -> bool {
    v == FALSE_VALUE || v == TRUE_VALUE
}

#[inline]
pub fn is_nil(v: Value) -> bool {
    v == NIL_VALUE
}

/// Get type of a value
pub fn get_type_id(v: Value) -> BuiltinTypeId {
    match v & TAG_MASK {
        TAG_INT => BuiltinTypeId::Int64,
        TAG_PTR => {
            // Read type from object header
            let ptr = untag_ptr(v).unwrap();
            let header = unsafe { &*(ptr as *const ObjectHeader) };
            // TODO: Map header.type_id to BuiltinTypeId
            BuiltinTypeId::Type
        }
        _ if v == FALSE_VALUE || v == TRUE_VALUE => BuiltinTypeId::Int8, // Bool is represented as Int8
        _ if v == NIL_VALUE => BuiltinTypeId::Void,
        _ => BuiltinTypeId::Void, // Unknown type
    }
}

/// Allocate a heap object with header
pub fn alloc_object(type_id: u32, size: usize) -> Value {
    let total_size = std::mem::size_of::<ObjectHeader>() + size;
    let layout = std::alloc::Layout::from_size_align(total_size, 8).unwrap();
    
    unsafe {
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            panic!("Out of memory");
        }
        
        // Write header
        let header = ptr as *mut ObjectHeader;
        (*header).type_id = type_id;
        (*header).size = size as u32;
        
        tag_ptr(ptr)
    }
}

/// Allocate a float
pub fn alloc_float(f: f64) -> Value {
    let obj = alloc_object(BuiltinTypeId::Any as u32, 8); // Using Any for floats temporarily
    let ptr = untag_ptr(obj).unwrap();
    unsafe {
        let data_ptr = ptr.add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
        *data_ptr = f;
    }
    obj
}

/// Extract float value
pub fn untag_float(v: Value) -> Option<f64> {
    if is_ptr(v) {
        let ptr = untag_ptr(v).unwrap();
        unsafe {
            let header = &*(ptr as *const ObjectHeader);
            if header.type_id == BuiltinTypeId::Any as u32 { // Using Any for floats temporarily
                let data_ptr = ptr.add(std::mem::size_of::<ObjectHeader>()) as *const f64;
                Some(*data_ptr)
            } else {
                None
            }
        }
    } else {
        None
    }
}

/// Allocate a string
pub fn alloc_string(s: &str) -> Value {
    let bytes = s.as_bytes();
    let obj = alloc_object(BuiltinTypeId::String as u32, bytes.len() + 1);
    let ptr = untag_ptr(obj).unwrap();
    unsafe {
        let data_ptr = ptr.add(std::mem::size_of::<ObjectHeader>()) as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, bytes.len());
        *data_ptr.add(bytes.len()) = 0; // Null terminator
    }
    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integers() {
        assert!(is_int(tag_int(42)));
        assert_eq!(untag_int(tag_int(42)), Some(42));
        assert_eq!(untag_int(tag_int(-1)), Some(-1));
        assert_eq!(untag_int(FALSE_VALUE), None);
    }

    #[test]
    fn test_booleans() {
        assert!(is_bool(tag_bool(true)));
        assert!(is_bool(tag_bool(false)));
        assert_eq!(untag_bool(tag_bool(true)), Some(true));
        assert_eq!(untag_bool(tag_bool(false)), Some(false));
    }

    #[test]
    fn test_floats() {
        let f = alloc_float(3.14);
        assert!(is_ptr(f));
        assert_eq!(untag_float(f), Some(3.14));
    }
}