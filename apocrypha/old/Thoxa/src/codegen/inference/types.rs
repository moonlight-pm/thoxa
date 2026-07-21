use crate::codegen::types::TypeId;
use std::fmt;
use std::collections::HashSet;

/// Represents types during inference
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferenceType {
    /// Concrete, known type
    Concrete(TypeId),
    
    /// Type variable (unknown type to be inferred)
    Variable(TypeVar),
    
    /// Generic type application (e.g., Box<T>, May<Int>)
    Application {
        constructor: TypeId,
        args: Vec<InferenceType>,
    },
    
    /// Function type
    Function {
        params: Vec<InferenceType>,
        ret: Box<InferenceType>,
    },
    
    /// Array type
    Array(Box<InferenceType>),
    
    /// Tuple type
    Tuple(Vec<InferenceType>),
}

/// Type variable for inference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVar {
    pub id: u32,
    pub name: Option<&'static str>,
    pub level: u32,  // For let-polymorphism
}

/// Type scheme with quantified variables (for polymorphism)
#[derive(Debug, Clone)]
pub struct TypeScheme {
    pub quantified: Vec<TypeVar>,
    pub body: InferenceType,
}

impl InferenceType {
    /// Check if a type variable occurs in this type (for occurs check)
    pub fn occurs(&self, var: &TypeVar) -> bool {
        match self {
            InferenceType::Variable(v) => v == var,
            InferenceType::Application { args, .. } => {
                args.iter().any(|arg| arg.occurs(var))
            }
            InferenceType::Function { params, ret } => {
                params.iter().any(|p| p.occurs(var)) || ret.occurs(var)
            }
            InferenceType::Array(elem) => elem.occurs(var),
            InferenceType::Tuple(elems) => elems.iter().any(|e| e.occurs(var)),
            InferenceType::Concrete(_) => false,
        }
    }
    
    /// Get all free type variables in this type
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut vars = HashSet::new();
        self.collect_free_vars(&mut vars);
        vars
    }
    
    fn collect_free_vars(&self, vars: &mut HashSet<TypeVar>) {
        match self {
            InferenceType::Variable(v) => {
                vars.insert(*v);
            }
            InferenceType::Application { args, .. } => {
                for arg in args {
                    arg.collect_free_vars(vars);
                }
            }
            InferenceType::Function { params, ret } => {
                for param in params {
                    param.collect_free_vars(vars);
                }
                ret.collect_free_vars(vars);
            }
            InferenceType::Array(elem) => elem.collect_free_vars(vars),
            InferenceType::Tuple(elems) => {
                for elem in elems {
                    elem.collect_free_vars(vars);
                }
            }
            InferenceType::Concrete(_) => {}
        }
    }
    
    /// Convert from parser AST type to inference type
    pub fn from_ast_type(ty: &crate::ast::Type, type_params: &[String]) -> Self {
        use crate::ast::Type as AstType;
        
        match ty {
            AstType::Named { name, args, .. } => {
                // Check if it's a type parameter
                if type_params.contains(name) && args.is_empty() {
                    // For now, create a fresh variable for each type param
                    // This will be properly handled during instantiation
                    InferenceType::Variable(TypeVar {
                        id: name.chars().map(|c| c as u32).sum(), // Temporary hack
                        name: None,
                        level: 0,
                    })
                } else if !args.is_empty() {
                    // Generic type application
                    let constructor = match name.as_str() {
                        "May" => TypeId::MAY,
                        "Trial" => TypeId::TRIAL,
                        _ => TypeId(1000), // Custom type - will be resolved later
                    };
                    
                    let arg_types = args.iter()
                        .map(|p| Self::from_ast_type(p, type_params))
                        .collect();
                    
                    InferenceType::Application { constructor, args: arg_types }
                } else {
                    // Map to concrete type
                    match name.as_str() {
                        "Int" => InferenceType::Concrete(TypeId::INT),
                        "String" => InferenceType::Concrete(TypeId::STRING),
                        "Bool" => InferenceType::Concrete(TypeId::BOOL),
                        "Float" => InferenceType::Concrete(TypeId::FLOAT),
                        "Char" => InferenceType::Concrete(TypeId::CHAR),
                        "Void" => InferenceType::Concrete(TypeId::VOID),
                        _ => {
                            // Custom type - will be resolved during constraint generation
                            InferenceType::Concrete(TypeId(1000))
                        }
                    }
                }
            }
            AstType::Function { params, ret, .. } => {
                let param_types = params.iter()
                    .map(|p| Self::from_ast_type(p, type_params))
                    .collect();
                let ret_type = Box::new(Self::from_ast_type(ret, type_params));
                
                InferenceType::Function {
                    params: param_types,
                    ret: ret_type,
                }
            }
            AstType::Array { elem, .. } => {
                InferenceType::Array(Box::new(Self::from_ast_type(elem, type_params)))
            }
            AstType::Tuple { elems, .. } => {
                // Tuple types - create a tuple type with element types
                let elem_types = elems.iter()
                    .map(|e| Self::from_ast_type(e, type_params))
                    .collect();
                InferenceType::Tuple(elem_types)
            }
            AstType::Union { .. } => {
                // Union types need special handling
                InferenceType::Concrete(TypeId(1001))
            }
        }
    }
    
    /// Convert inference type back to TypeId (after inference is complete)
    pub fn to_type_id(&self) -> Result<TypeId, String> {
        match self {
            InferenceType::Concrete(id) => Ok(*id),
            InferenceType::Variable(v) => Err(format!("Unresolved type variable: {:?}", v)),
            InferenceType::Application { constructor, args } => {
                // For now, just return the constructor
                // TODO: Handle generic type applications properly
                Ok(*constructor)
            }
            InferenceType::Array(_) => Ok(TypeId(2000)), // Array type
            InferenceType::Tuple(_) => Ok(TypeId(2001)), // Tuple type
            InferenceType::Function { .. } => Ok(TypeId(2002)), // Function type
        }
    }
}

impl fmt::Display for InferenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferenceType::Concrete(id) => write!(f, "{:?}", id),
            InferenceType::Variable(v) => {
                if let Some(name) = v.name {
                    write!(f, "{}", name)
                } else {
                    write!(f, "?{}", v.id)
                }
            }
            InferenceType::Application { constructor, args } => {
                write!(f, "{:?}<", constructor)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
            InferenceType::Function { params, ret } => {
                write!(f, "(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") -> {}", ret)
            }
            InferenceType::Array(elem) => write!(f, "[{}]", elem),
            InferenceType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl TypeScheme {
    /// Instantiate a type scheme with fresh type variables
    pub fn instantiate(&self, fresh_vars: &mut dyn FnMut() -> TypeVar) -> InferenceType {
        let mut substitution = HashMap::new();
        
        // Create fresh variables for each quantified variable
        for &var in &self.quantified {
            substitution.insert(var, InferenceType::Variable(fresh_vars()));
        }
        
        // Apply substitution to body
        self.substitute_type(&self.body, &substitution)
    }
    
    fn substitute_type(&self, ty: &InferenceType, subst: &HashMap<TypeVar, InferenceType>) -> InferenceType {
        match ty {
            InferenceType::Variable(v) => {
                subst.get(v).cloned().unwrap_or_else(|| ty.clone())
            }
            InferenceType::Application { constructor, args } => {
                InferenceType::Application {
                    constructor: *constructor,
                    args: args.iter().map(|arg| self.substitute_type(arg, subst)).collect(),
                }
            }
            InferenceType::Function { params, ret } => {
                InferenceType::Function {
                    params: params.iter().map(|p| self.substitute_type(p, subst)).collect(),
                    ret: Box::new(self.substitute_type(ret, subst)),
                }
            }
            InferenceType::Array(elem) => {
                InferenceType::Array(Box::new(self.substitute_type(elem, subst)))
            }
            InferenceType::Tuple(elems) => {
                InferenceType::Tuple(elems.iter().map(|e| self.substitute_type(e, subst)).collect())
            }
            InferenceType::Concrete(_) => ty.clone(),
        }
    }
}

use std::collections::HashMap;