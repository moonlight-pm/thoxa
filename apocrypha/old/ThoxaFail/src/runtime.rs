/// Runtime type information support for Thoxa
/// 
/// This module implements the runtime type system that enables
/// polymorphic programming with the Any type.

use crate::types::Type;

/// Built-in type IDs for the hybrid type tag system
/// These are used when bit 63 = 0 in the type tag
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeId {
    Void = 0x0000_0000_0000_0000,
    Int8 = 0x0000_0000_0000_0001,
    Int16 = 0x0000_0000_0000_0002,
    Int32 = 0x0000_0000_0000_0003,
    Int64 = 0x0000_0000_0000_0004,
    Uint8 = 0x0000_0000_0000_0005,
    Uint16 = 0x0000_0000_0000_0006,
    Uint32 = 0x0000_0000_0000_0007,
    Uint64 = 0x0000_0000_0000_0008,
    String = 0x0000_0000_0000_0010,
    Type = 0x0000_0000_0000_0011,
    Any = 0x0000_0000_0000_0012,
    Array = 0x0000_0000_0000_0013,
}

/// Runtime representation of type information
/// For custom types, this struct is pointed to by the type tag
#[repr(C)]
pub struct TypeInfo {
    /// Type name as a C string
    pub name: *const u8,
    /// Size in bytes
    pub size: usize,
    /// Pointer to conversion function table
    pub conversions: *const ConversionTable,
    /// Pointer to method table
    pub methods: *const MethodTable,
}

/// Placeholder for conversion function table
#[repr(C)]
pub struct ConversionTable {
    // TODO: Define conversion function table structure
}

/// Placeholder for method table
#[repr(C)]
pub struct MethodTable {
    // TODO: Define method table structure
}

/// Type tag operations
impl BuiltinTypeId {
    /// Check if a type tag represents a built-in type
    pub fn is_builtin(tag: u64) -> bool {
        (tag & 0x8000_0000_0000_0000) == 0
    }
    
    /// Get the built-in type ID from a tag (assumes is_builtin is true)
    pub fn from_tag(tag: u64) -> Option<BuiltinTypeId> {
        if !Self::is_builtin(tag) {
            return None;
        }
        
        match tag {
            0x0000_0000_0000_0000 => Some(BuiltinTypeId::Void),
            0x0000_0000_0000_0001 => Some(BuiltinTypeId::Int8),
            0x0000_0000_0000_0002 => Some(BuiltinTypeId::Int16),
            0x0000_0000_0000_0003 => Some(BuiltinTypeId::Int32),
            0x0000_0000_0000_0004 => Some(BuiltinTypeId::Int64),
            0x0000_0000_0000_0005 => Some(BuiltinTypeId::Uint8),
            0x0000_0000_0000_0006 => Some(BuiltinTypeId::Uint16),
            0x0000_0000_0000_0007 => Some(BuiltinTypeId::Uint32),
            0x0000_0000_0000_0008 => Some(BuiltinTypeId::Uint64),
            0x0000_0000_0000_0010 => Some(BuiltinTypeId::String),
            0x0000_0000_0000_0011 => Some(BuiltinTypeId::Type),
            0x0000_0000_0000_0012 => Some(BuiltinTypeId::Any),
            0x0000_0000_0000_0013 => Some(BuiltinTypeId::Array),
            _ => None,
        }
    }
    
    /// Convert a Thoxa Type to a built-in type ID
    pub fn from_type(ty: &Type) -> Option<BuiltinTypeId> {
        match ty {
            Type::Void => Some(BuiltinTypeId::Void),
            Type::Int { signed: true, bits: 8 } => Some(BuiltinTypeId::Int8),
            Type::Int { signed: true, bits: 16 } => Some(BuiltinTypeId::Int16),
            Type::Int { signed: true, bits: 32 } => Some(BuiltinTypeId::Int32),
            Type::Int { signed: true, bits: 64 } => Some(BuiltinTypeId::Int64),
            Type::Int { signed: false, bits: 8 } => Some(BuiltinTypeId::Uint8),
            Type::Int { signed: false, bits: 16 } => Some(BuiltinTypeId::Uint16),
            Type::Int { signed: false, bits: 32 } => Some(BuiltinTypeId::Uint32),
            Type::Int { signed: false, bits: 64 } => Some(BuiltinTypeId::Uint64),
            Type::String => Some(BuiltinTypeId::String),
            Type::Type => Some(BuiltinTypeId::Type),
            Type::Any => Some(BuiltinTypeId::Any),
            Type::Array { .. } => Some(BuiltinTypeId::Array),
            _ => None, // Custom types
        }
    }
}

/// Create a type tag for a custom type (TypeInfo pointer)
pub fn custom_type_tag(type_info: *const TypeInfo) -> u64 {
    0x8000_0000_0000_0000 | (type_info as u64)
}

/// Extract TypeInfo pointer from a custom type tag
pub fn type_info_from_tag(tag: u64) -> Option<*const TypeInfo> {
    if BuiltinTypeId::is_builtin(tag) {
        None
    } else {
        Some((tag & 0x7FFF_FFFF_FFFF_FFFF) as *const TypeInfo)
    }
}