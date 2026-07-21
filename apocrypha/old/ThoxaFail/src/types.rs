use std::collections::HashMap;

/// Represents types in the Thoxa type system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// The type of types
    Type,
    
    /// Void type - for functions with no meaningful return
    Void,
    
    /// String type - UTF-8 encoded text
    String,
    
    /// Integer types
    Int { signed: bool, bits: u8 },
    
    /// User-defined struct types
    Struct {
        name: String,
        members: HashMap<String, MemberType>,
        methods: HashMap<String, Type>, // Method name -> Function type
        conversions: HashMap<String, Type>, // Target type name -> Function type
    },
    
    /// Function types
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
        variadic: Option<Box<Type>>,  // Type of variadic parameter if present
    },
    
    /// Array types
    Array {
        element_type: Box<Type>,
        size: Option<usize>, // None for dynamic arrays
    },
    
    /// Any type - can hold any value
    Any,
    
    /// Unknown type (for error recovery)
    Unknown,
    
    /// Type variable (for future type inference)
    TypeVar(String),
}

/// Type of a module member
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberType {
    pub ty: Type,
    pub exported: bool,
}

impl Type {
    /// Create a signed integer type
    pub fn int(bits: u8) -> Self {
        Type::Int { signed: true, bits }
    }
    
    /// Create an unsigned integer type
    pub fn uint(bits: u8) -> Self {
        Type::Int { signed: false, bits }
    }
    
    /// Get the default integer type (Int64)
    pub fn default_int() -> Self {
        Type::int(64)
    }
    
    /// Check if this type is numeric
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int { .. })
    }
    
    /// Check if this type is equal to another, ignoring type variables
    pub fn unify(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            (Type::TypeVar(_), _) | (_, Type::TypeVar(_)) => true,
            _ => self == other,
        }
    }
    
    /// Get a human-readable name for the type
    pub fn name(&self) -> String {
        match self {
            Type::Type => "Type".to_string(),
            Type::Void => "Void".to_string(),
            Type::String => "String".to_string(),
            Type::Any => "Any".to_string(),
            Type::Int { signed: true, bits: 8 } => "Int8".to_string(),
            Type::Int { signed: true, bits: 16 } => "Int16".to_string(),
            Type::Int { signed: true, bits: 32 } => "Int32".to_string(),
            Type::Int { signed: true, bits: 64 } => "Int64".to_string(),
            Type::Int { signed: false, bits: 8 } => "Uint8".to_string(),
            Type::Int { signed: false, bits: 16 } => "Uint16".to_string(),
            Type::Int { signed: false, bits: 32 } => "Uint32".to_string(),
            Type::Int { signed: false, bits: 64 } => "Uint64".to_string(),
            Type::Int { signed, bits } => format!("{}int{}", if *signed { "" } else { "u" }, bits),
            Type::Struct { name, .. } => name.clone(),
            Type::Function { params, return_type, variadic } => {
                let mut param_names: Vec<String> = params.iter().map(|t| t.name()).collect();
                if let Some(var_type) = variadic {
                    param_names.push(format!("...{}", var_type.name()));
                }
                format!("({}) -> {}", param_names.join(", "), return_type.name())
            }
            Type::Array { element_type, size } => {
                match size {
                    Some(n) => format!("[{}; {}]", element_type.name(), n),
                    None => format!("[{}]", element_type.name()),
                }
            }
            Type::Unknown => "?".to_string(),
            Type::TypeVar(name) => format!("'{}", name),
        }
    }
    
    /// Look up a method on this type
    pub fn get_method(&self, name: &str) -> Option<Type> {
        match self {
            Type::Struct { methods, .. } => methods.get(name).cloned(),
            Type::String => {
                // Built-in String methods
                match name {
                    "toString" => {
                        // String.toString() returns itself
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            Type::Int { .. } => {
                // Built-in integer methods
                match name {
                    "toString" => {
                        // Int.toString() converts to string
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            Type::Array { element_type: _, .. } => {
                // Built-in array methods
                match name {
                    "length" => {
                        // Array.length() returns Int64
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::int(64)),
                            variadic: None,
                        })
                    }
                    "toString" => {
                        // Array.toString() converts to string representation
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
    
    /// Check if this type has a method
    pub fn has_method(&self, name: &str) -> bool {
        self.get_method(name).is_some()
    }
    
    /// Look up a conversion on this type
    pub fn get_conversion(&self, target_type: &str) -> Option<Type> {
        match self {
            Type::Struct { conversions, .. } => conversions.get(target_type).cloned(),
            // Built-in type conversions
            Type::Int { .. } => {
                match target_type {
                    "String" => {
                        // Int -> String conversion
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            Type::String => {
                match target_type {
                    "String" => {
                        // String -> String (identity)
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            Type::Any => {
                // Any can be converted to any type with runtime checking
                match target_type {
                    "Int64" => {
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::int(64)),
                            variadic: None,
                        })
                    }
                    "String" => {
                        Some(Type::Function {
                            params: vec![],
                            return_type: Box::new(Type::String),
                            variadic: None,
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
    
    /// Check if this type can be converted to another type
    pub fn can_convert_to(&self, target_type: &str) -> bool {
        self.get_conversion(target_type).is_some()
    }
    
    /// Find a conversion path to the target type, including chained conversions
    /// Returns None if no path exists, or Some(vec![]) for identity conversion
    pub fn find_conversion_path(&self, target_type_name: &str, type_env: &TypeEnv) -> Option<Vec<String>> {
        // Check for identity conversion
        if self.name() == target_type_name {
            return Some(vec![]);
        }
        
        // Check for direct conversion
        if self.can_convert_to(target_type_name) {
            return Some(vec![target_type_name.to_string()]);
        }
        
        // Try to find a chained conversion path (A -> B -> C)
        // For now, limit to 2 hops to avoid infinite loops
        match self {
            Type::Struct { conversions, .. } => {
                for (intermediate_type_name, _) in conversions {
                    // Look up the intermediate type
                    if let Some(intermediate_type) = type_env.lookup_type(intermediate_type_name) {
                        // Check if intermediate type can convert to target
                        if intermediate_type.can_convert_to(target_type_name) {
                            return Some(vec![
                                intermediate_type_name.clone(),
                                target_type_name.to_string(),
                            ]);
                        }
                    }
                }
            }
            _ => {
                // For built-in types, try common intermediate conversions
                // For example: Int -> String -> SomeType
                if self.can_convert_to("String") {
                    if let Some(string_type) = type_env.lookup_type("String").or_else(|| Some(Type::String)) {
                        if string_type.can_convert_to(target_type_name) {
                            return Some(vec!["String".to_string(), target_type_name.to_string()]);
                        }
                    }
                }
            }
        }
        
        None
    }
}

/// Type environment for tracking variable and function types
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Variable types in current scope
    pub variables: HashMap<String, Type>,
    
    /// Function types in current scope
    pub functions: HashMap<String, Type>,
    
    /// User-defined type definitions
    pub types: HashMap<String, Type>,
    
    /// Parent scope (for nested scopes)
    pub parent: Option<Box<TypeEnv>>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashMap::new(),
            types: HashMap::new(),
            parent: None,
        }
    }
    
    /// Create a child scope
    pub fn child(parent: TypeEnv) -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashMap::new(),
            types: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }
    
    /// Look up a variable type
    pub fn lookup_var(&self, name: &str) -> Option<Type> {
        self.variables.get(name).cloned()
            .or_else(|| self.parent.as_ref()?.lookup_var(name))
    }
    
    /// Look up a function type
    pub fn lookup_func(&self, name: &str) -> Option<Type> {
        self.functions.get(name).cloned()
            .or_else(|| self.parent.as_ref()?.lookup_func(name))
    }
    
    /// Look up a type definition
    pub fn lookup_type(&self, name: &str) -> Option<Type> {
        self.types.get(name).cloned()
            .or_else(|| self.parent.as_ref()?.lookup_type(name))
    }
    
    /// Insert a variable type
    pub fn insert_var(&mut self, name: String, ty: Type) {
        self.variables.insert(name, ty);
    }
    
    /// Insert a function type
    pub fn insert_func(&mut self, name: String, ty: Type) {
        self.functions.insert(name, ty);
    }
    
    /// Insert a type definition
    pub fn insert_type(&mut self, name: String, ty: Type) {
        self.types.insert(name, ty);
    }
}