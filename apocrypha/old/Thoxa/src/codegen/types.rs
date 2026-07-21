//! Type registry and type information management

use std::collections::HashMap;

/// Unique identifier for a type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

// Pre-defined TypeIds for built-in types
impl TypeId {
    // Signed integers
    pub const INT8: TypeId = TypeId(0);
    pub const INT16: TypeId = TypeId(1);
    pub const INT32: TypeId = TypeId(2);
    pub const INT64: TypeId = TypeId(3);
    pub const INT: TypeId = TypeId(3);  // Same as INT64
    
    // Unsigned integers
    pub const UINT8: TypeId = TypeId(4);
    pub const UINT16: TypeId = TypeId(5);
    pub const UINT32: TypeId = TypeId(6);
    pub const UINT64: TypeId = TypeId(7);
    pub const UINT: TypeId = TypeId(7);  // Same as UINT64
    
    // Floating point
    pub const FLOAT32: TypeId = TypeId(8);
    pub const FLOAT64: TypeId = TypeId(9);
    pub const FLOAT: TypeId = TypeId(9);  // Same as FLOAT64
    
    // Other primitives
    pub const BOOL: TypeId = TypeId(10);
    pub const CHAR: TypeId = TypeId(11);
    pub const STRING: TypeId = TypeId(12);
    pub const VOID: TypeId = TypeId(13);
    pub const POINTER: TypeId = TypeId(14);  // Generic pointer type
    
    // Trial type (generic error handling)
    // We'll use POINTER for now since Trial values are heap-allocated
    // In the future, we'll need a way to track type parameters
    pub const TRIAL: TypeId = TypeId(15);  // Base Trial type
    
    // May type (optional values)
    pub const MAY: TypeId = TypeId(16);  // Base May type
    
    // Function type (for first-class functions)
    pub const FUNCTION: TypeId = TypeId(17);  // Base function type
    
    // First user-defined type ID
    const FIRST_USER_TYPE: u32 = 100;
    
    /// Convert TypeId to u16 for runtime type tags
    pub fn to_u16(&self) -> u16 {
        self.0 as u16
    }
}

/// Kind of type 
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Primitive,    // Built-in types like Int, Float, Bool
    Unit,         // User-defined unit types
    Array,        // Array types
    Function,     // Function types
    Trial,        // Error handling type
    Union,        // Union types (sum types)
    Tuple,        // Tuple types (product types)
}

/// Information about a Thoxa type
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Unique ID for this type
    pub id: TypeId,
    /// Human-readable name
    pub name: String,
    /// Size in bytes (for heap allocation)
    pub size: usize,
    /// Whether this is a built-in type
    pub is_builtin: bool,
    /// Module path where this type is defined (None for built-ins)
    pub module_path: Option<String>,
    /// QBE type representation (usually 'l' for pointer)
    pub qbe_type: &'static str,
    /// For generic types: the type parameters (e.g., Trial<T,E> has [T, E])
    pub type_params: Option<Vec<TypeId>>,
    /// Kind of type
    pub kind: TypeKind,
    /// For units: field information (name, type, is_mutable)
    pub fields: Option<Vec<(String, TypeId, bool)>>,
    /// For units: constructor parameter information (name, type)
    pub constructor_params: Option<Vec<(String, TypeId)>>,
    /// For unions: variant types that are part of the union
    pub union_variants: Option<Vec<TypeId>>,
    /// For tuples: element types
    pub tuple_elements: Option<Vec<TypeId>>,
}

/// Registry of all types in the program
pub struct TypeRegistry {
    /// All registered types
    types: HashMap<TypeId, TypeInfo>,
    /// Next available type ID for user types
    next_id: u32,
}

impl TypeRegistry {
    /// Create a new type registry with built-in types
    pub fn new() -> Self {
        let mut types = HashMap::new();
        
        // Register all built-in types with their pre-defined IDs
        types.insert(TypeId::INT8, TypeInfo {
            id: TypeId::INT8,
            name: "Int8".to_string(),
            size: 1,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::INT16, TypeInfo {
            id: TypeId::INT16,
            name: "Int16".to_string(),
            size: 2,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::INT32, TypeInfo {
            id: TypeId::INT32,
            name: "Int32".to_string(),
            size: 4,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::INT64, TypeInfo {
            id: TypeId::INT64,
            name: "Int64".to_string(),
            size: 8,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        // Note: INT uses same TypeId as INT64, so no separate entry needed
        
        types.insert(TypeId::UINT8, TypeInfo {
            id: TypeId::UINT8,
            name: "UInt8".to_string(),
            size: 1,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::UINT16, TypeInfo {
            id: TypeId::UINT16,
            name: "UInt16".to_string(),
            size: 2,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::UINT32, TypeInfo {
            id: TypeId::UINT32,
            name: "UInt32".to_string(),
            size: 4,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::UINT64, TypeInfo {
            id: TypeId::UINT64,
            name: "UInt64".to_string(),
            size: 8,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::FLOAT32, TypeInfo {
            id: TypeId::FLOAT32,
            name: "Float32".to_string(),
            size: 4,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::FLOAT64, TypeInfo {
            id: TypeId::FLOAT64,
            name: "Float64".to_string(),
            size: 8,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::BOOL, TypeInfo {
            id: TypeId::BOOL,
            name: "Bool".to_string(),
            size: 1,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::CHAR, TypeInfo {
            id: TypeId::CHAR,
            name: "Char".to_string(),
            size: 4,  // Unicode scalar value (u32)
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::STRING, TypeInfo {
            id: TypeId::STRING,
            name: "String".to_string(),
            size: 16,  // String header (ptr + len)
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::VOID, TypeInfo {
            id: TypeId::VOID,
            name: "Void".to_string(),
            size: 0,
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        types.insert(TypeId::POINTER, TypeInfo {
            id: TypeId::POINTER,
            name: "Ptr".to_string(),
            size: 8,  // 64-bit pointer
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        // Note: Trial is a generic type, specific instances will be created on demand
        types.insert(TypeId::TRIAL, TypeInfo {
            id: TypeId::TRIAL,
            name: "Trial".to_string(),
            size: 8,  // Base size (pointer to heap-allocated Trial)
            is_builtin: true,
            module_path: None,
            qbe_type: "l",
            type_params: None,  // Base type, instances will have type params
            kind: TypeKind::Trial,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        });
        
        TypeRegistry {
            types,
            next_id: TypeId::FIRST_USER_TYPE,
        }
    }
    
    /// Get a type by ID
    pub fn get(&self, id: TypeId) -> Option<&TypeInfo> {
        self.types.get(&id)
    }
    
    /// Get type ID by name
    pub fn get_type_id(&self, name: &str) -> Option<TypeId> {
        for (id, info) in &self.types {
            if info.name == name {
                return Some(*id);
            }
        }
        None
    }
    
    /// Get type information by name
    pub fn get_by_name(&self, name: &str) -> Option<&TypeInfo> {
        self.types.values().find(|info| info.name == name)
    }
    
    /// Get the name of a type by its ID
    pub fn get_type_name(&self, type_id: TypeId) -> Option<String> {
        self.types.get(&type_id).map(|info| info.name.clone())
    }
    
    /// Find a type ID by name
    pub fn find_by_name(&self, name: &str) -> Option<TypeId> {
        self.types.iter()
            .find(|(_, info)| info.name == name)
            .map(|(id, _)| *id)
    }
    
    /// Check if a type is an array and get its element type
    pub fn get_array_element_type(&self, ty: TypeId) -> Option<TypeId> {
        if let Some(info) = self.get(ty) {
            if info.kind == TypeKind::Array {
                if let Some(ref params) = info.type_params {
                    if !params.is_empty() {
                        return Some(params[0]);
                    }
                }
            }
        }
        None
    }
    
    /// Check if a type needs reference counting
    pub fn needs_refcount(&self, type_id: TypeId) -> bool {
        // Check if this is a heap-allocated type
        if let Some(info) = self.types.get(&type_id) {
            match info.kind {
                TypeKind::Unit => true,  // Units are heap-allocated
                TypeKind::Trial => true, // Trial values are heap-allocated
                TypeKind::Union => true, // Unions are heap-allocated
                TypeKind::Array => true, // Arrays are heap-allocated
                _ => false,              // Primitives are stack-allocated
            }
        } else {
            // Unknown types or special types
            match type_id {
                TypeId::POINTER => true,  // Pointers need refcounting
                TypeId::STRING => true,   // Strings are heap-allocated
                _ => false,
            }
        }
    }
    
    /// Check if a type is a variant of a union type
    pub fn is_variant_of(&self, variant_type: TypeId, union_type: TypeId) -> bool {
        if let Some(union_info) = self.types.get(&union_type) {
            if union_info.kind == TypeKind::Union {
                if let Some(ref variants) = union_info.union_variants {
                    return variants.contains(&variant_type);
                }
            }
        }
        false
    }
    
    /// Find all unions that contain a given variant type
    pub fn find_unions_containing(&self, variant_type: TypeId) -> Vec<TypeId> {
        let mut result = Vec::new();
        for (id, info) in &self.types {
            if info.kind == TypeKind::Union {
                if let Some(ref variants) = info.union_variants {
                    if variants.contains(&variant_type) {
                        result.push(*id);
                    }
                }
            }
        }
        result
    }
    
    /// Register a user-defined type
    pub fn register_type(&mut self, name: String, size: usize, module_path: Option<String>) -> TypeId {
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        let info = TypeInfo {
            id,
            name,
            size,
            is_builtin: false,
            module_path,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Primitive,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register a unit type with fields and constructor parameters
    pub fn register_unit_type(&mut self, name: String, fields: Vec<(String, TypeId, bool)>, constructor_params: Vec<(String, TypeId)>) -> TypeId {
        // Check if this type already exists
        for (id, info) in &self.types {
            if info.name == name && info.kind == TypeKind::Unit {
                return *id;
            }
        }
        
        // Create a new type ID
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Calculate size based on fields (simplified - actual size depends on alignment)
        let size = 16 + fields.len() * 8;  // Header + field pointers
        
        let info = TypeInfo {
            id,
            name,
            size,
            is_builtin: false,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Unit,
            fields: Some(fields),
            constructor_params: if constructor_params.is_empty() { None } else { Some(constructor_params) },
            union_variants: None,
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register a union type (e.g., Shape = Circle | Rectangle)
    pub fn register_union_type(&mut self, name: String, variants: Vec<TypeId>, _type_params: Option<Vec<String>>) -> TypeId {
        // Check if this union already exists
        for (id, info) in &self.types {
            if info.name == name && info.kind == TypeKind::Union {
                return *id;
            }
        }
        
        // Create a new type ID
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Unions are heap-allocated and use a discriminated union structure
        // Size = tag (2 bytes) + padding (2 bytes) + largest variant (8 bytes min)
        let size = 12;  // Minimum size for discriminated union
        
        let info = TypeInfo {
            id,
            name,
            size,
            is_builtin: false,
            module_path: None,
            qbe_type: "l",  // Pointer type
            type_params: None,  // For now, store as strings, convert later
            kind: TypeKind::Union,
            fields: None,
            constructor_params: None,
            union_variants: Some(variants),
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register a tuple type (e.g., (Int, String, Bool))
    pub fn register_tuple_type(&mut self, element_types: Vec<TypeId>) -> TypeId {
        // Check if this exact tuple already exists
        for (id, info) in &self.types {
            if info.kind == TypeKind::Tuple {
                if let Some(ref elems) = info.tuple_elements {
                    if elems == &element_types {
                        return *id;
                    }
                }
            }
        }
        
        // Create a new tuple type
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Build name like "(Int, String, Bool)"
        let elem_names: Vec<String> = element_types.iter()
            .filter_map(|tid| self.get(*tid))
            .map(|info| info.name.clone())
            .collect();
        let full_name = format!("({})", elem_names.join(", "));
        
        let info = TypeInfo {
            id,
            name: full_name,
            size: 8,  // Tuples are heap-allocated
            is_builtin: false,
            module_path: None,
            qbe_type: "l",
            type_params: None,
            kind: TypeKind::Tuple,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: Some(element_types),
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register a function type (e.g., (Int, Int) -> Int)
    pub fn register_function_type(&mut self, param_types: Vec<TypeId>, return_type: TypeId) -> TypeId {
        // Check if this exact function type already exists
        for (id, info) in &self.types {
            if info.kind == TypeKind::Function {
                if let Some(ref params) = info.type_params {
                    // Function types store params as [param1, param2, ..., return_type]
                    if params.len() == param_types.len() + 1 {
                        let stored_params = &params[..params.len() - 1];
                        let stored_return = params[params.len() - 1];
                        if stored_params == param_types.as_slice() && stored_return == return_type {
                            return *id;
                        }
                    }
                }
            }
        }
        
        // Create a new function type
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Build name like "(Int, String) -> Bool"
        let param_names: Vec<String> = param_types.iter()
            .filter_map(|tid| self.get(*tid))
            .map(|info| info.name.clone())
            .collect();
        let return_name = self.get(return_type)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| format!("TypeId({})", return_type.0));
        
        let full_name = if param_names.is_empty() {
            format!("() -> {}", return_name)
        } else {
            format!("({}) -> {}", param_names.join(", "), return_name)
        };
        
        // Store all type info in type_params: [param_types..., return_type]
        let mut all_types = param_types;
        all_types.push(return_type);
        
        let info = TypeInfo {
            id,
            name: full_name,
            size: 8,  // Function pointers are 8 bytes
            is_builtin: false,
            module_path: None,
            qbe_type: "l",
            type_params: Some(all_types),
            kind: TypeKind::Function,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register an array type (e.g., [Int])
    pub fn register_array_type(&mut self, element_type: TypeId) -> TypeId {
        // Check if this exact array type already exists
        for (id, info) in &self.types {
            if info.kind == TypeKind::Array {
                if let Some(ref params) = info.type_params {
                    if params.len() == 1 && params[0] == element_type {
                        return *id;
                    }
                }
            }
        }
        
        // Create a new array type
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Build name like "[Int]"
        let elem_name = self.get(element_type)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| format!("TypeId({})", element_type.0));
        let full_name = format!("[{}]", elem_name);
        
        let info = TypeInfo {
            id,
            name: full_name,
            size: 8,  // Arrays are heap-allocated pointers
            is_builtin: false,
            module_path: None,
            qbe_type: "l",
            type_params: Some(vec![element_type]),  // Store element type as type parameter
            kind: TypeKind::Array,
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
    
    /// Register a generic type instance (e.g., Trial<Int, String>)
    pub fn register_generic_instance(&mut self, base_name: String, type_params: Vec<TypeId>) -> TypeId {
        // Check if this exact instance already exists
        for (id, info) in &self.types {
            if info.name.starts_with(&base_name) {
                if let Some(ref params) = info.type_params {
                    if params == &type_params {
                        return *id;
                    }
                }
            }
        }
        
        // Create a new instance
        let id = TypeId(self.next_id);
        self.next_id += 1;
        
        // Build name like "Trial<Int,String>"
        let param_names: Vec<String> = type_params.iter()
            .filter_map(|tid| self.get(*tid))
            .map(|info| info.name.clone())
            .collect();
        let full_name = format!("{}<{}>", base_name, param_names.join(","));
        
        let info = TypeInfo {
            id,
            name: full_name,
            size: 8,  // All generic types are heap-allocated pointers
            is_builtin: false,
            module_path: None,
            qbe_type: "l",
            type_params: Some(type_params),
            kind: if base_name == "Trial" { TypeKind::Trial } else { TypeKind::Primitive },
            fields: None,
            constructor_params: None,
            union_variants: None,
            tuple_elements: None,
        };
        
        self.types.insert(id, info);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_builtin_types() {
        let registry = TypeRegistry::new();
        
        // Check that built-in type IDs are accessible
        let int_info = registry.get(TypeId::INT).unwrap();
        let float_info = registry.get(TypeId::FLOAT).unwrap();
        let bool_info = registry.get(TypeId::BOOL).unwrap();
        let string_info = registry.get(TypeId::STRING).unwrap();
        let void_info = registry.get(TypeId::VOID).unwrap();
        
        // Check sizes
        assert_eq!(int_info.size, 8);
        assert_eq!(float_info.size, 8);
        assert_eq!(bool_info.size, 1);
        assert_eq!(string_info.size, 16);
        assert_eq!(void_info.size, 0);
        
        // Check that they're marked as built-in
        assert!(int_info.is_builtin);
        assert!(float_info.is_builtin);
        assert!(bool_info.is_builtin);
        assert!(string_info.is_builtin);
        assert!(void_info.is_builtin);
    }
    
    #[test]
    fn test_register_custom_type() {
        let mut registry = TypeRegistry::new();
        
        // Register a custom type without module path
        let point_id = registry.register_type("Point".to_string(), 16, None);
        
        // Get by ID
        let point_info = registry.get(point_id).unwrap();
        assert_eq!(point_info.name, "Point");
        assert_eq!(point_info.size, 16);
        assert!(!point_info.is_builtin);
        assert!(point_info.module_path.is_none());
        
        // Register a custom type with module path
        let rect_id = registry.register_type(
            "Rectangle".to_string(), 
            32, 
            Some("geometry.shapes".to_string())
        );
        
        let rect_info = registry.get(rect_id).unwrap();
        assert_eq!(rect_info.name, "Rectangle");
        assert_eq!(rect_info.module_path, Some("geometry.shapes".to_string()));
    }
    
    #[test]
    fn test_type_ids_are_unique() {
        let mut registry = TypeRegistry::new();
        
        // Register multiple types
        let type1 = registry.register_type("Type1".to_string(), 8, None);
        let type2 = registry.register_type("Type2".to_string(), 8, None);
        let type3 = registry.register_type("Type3".to_string(), 16, None);
        
        // All should have different IDs
        assert_ne!(type1, type2);
        assert_ne!(type2, type3);
        assert_ne!(type1, type3);
    }
    
    #[test]
    fn test_all_builtin_types() {
        let registry = TypeRegistry::new();
        
        // Test accessing various bit-width integers directly
        let int8_info = registry.get(TypeId::INT8).unwrap();
        assert_eq!(int8_info.size, 1);
        
        let int32_info = registry.get(TypeId::INT32).unwrap();
        assert_eq!(int32_info.size, 4);
        
        let uint64_info = registry.get(TypeId::UINT64).unwrap();
        assert_eq!(uint64_info.size, 8);
        
        let float32_info = registry.get(TypeId::FLOAT32).unwrap();
        assert_eq!(float32_info.size, 4);
        
        // Test that Int is same as Int64
        assert_eq!(TypeId::INT, TypeId::INT64);
        let int_info = registry.get(TypeId::INT).unwrap();
        assert_eq!(int_info.size, 8);
    }
}