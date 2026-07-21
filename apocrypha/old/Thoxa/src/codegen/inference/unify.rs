use super::types::{InferenceType, TypeVar};
use super::constraints::{Constraint, ConstraintSet};
use crate::codegen::types::TypeId;
use crate::error::Error;
use std::collections::HashMap;

/// Type substitution mapping variables to types
pub type Substitution = HashMap<TypeVar, InferenceType>;

/// Unification solver for type constraints
pub struct Unifier {
    substitutions: Substitution,
    errors: Vec<String>,
}

impl Unifier {
    pub fn new() -> Self {
        Self {
            substitutions: HashMap::new(),
            errors: Vec::new(),
        }
    }
    
    /// Solve a set of constraints
    pub fn solve(&mut self, constraints: ConstraintSet) -> Result<Substitution, Error> {
        for constraint in constraints.constraints {
            if let Err(e) = self.solve_constraint(constraint) {
                self.errors.push(e.to_string());
                // Continue solving to find more errors
            }
        }
        
        if !self.errors.is_empty() {
            return Err(Error::Codegen(format!(
                "Type inference failed:\n{}",
                self.errors.join("\n")
            )));
        }
        
        Ok(self.substitutions.clone())
    }
    
    fn solve_constraint(&mut self, constraint: Constraint) -> Result<(), Error> {
        match constraint {
            Constraint::Equal(t1, t2, span) => {
                self.unify(t1, t2).map_err(|e| {
                    Error::Codegen(format!("Type mismatch at {:?}: {}", span, e))
                })
            }
            
            Constraint::IsNumeric(ty, span) => {
                let resolved = self.resolve(ty);
                match resolved {
                    InferenceType::Concrete(TypeId::INT) |
                    InferenceType::Concrete(TypeId::FLOAT) => Ok(()),
                    InferenceType::Variable(v) => {
                        // Default to Int for unresolved numeric types
                        self.substitutions.insert(v, InferenceType::Concrete(TypeId::INT));
                        Ok(())
                    }
                    _ => Err(Error::Codegen(format!(
                        "Type {:?} does not support numeric operations at {:?}",
                        resolved, span
                    )))
                }
            }
            
            Constraint::IsComparable(ty, span) => {
                let resolved = self.resolve(ty);
                match resolved {
                    InferenceType::Concrete(TypeId::INT) |
                    InferenceType::Concrete(TypeId::FLOAT) |
                    InferenceType::Concrete(TypeId::STRING) |
                    InferenceType::Concrete(TypeId::CHAR) |
                    InferenceType::Concrete(TypeId::BOOL) => Ok(()),
                    InferenceType::Variable(v) => {
                        // Default to Int for unresolved comparable types
                        self.substitutions.insert(v, InferenceType::Concrete(TypeId::INT));
                        Ok(())
                    }
                    _ => Err(Error::Codegen(format!(
                        "Type {:?} does not support comparison at {:?}",
                        resolved, span
                    )))
                }
            }
            
            Constraint::IsTrial { trial_type, pass_type, fail_type, span } => {
                let expected = InferenceType::Application {
                    constructor: TypeId::TRIAL,
                    args: vec![pass_type, fail_type],
                };
                self.unify(trial_type, expected).map_err(|e| {
                    Error::Codegen(format!("Trial type mismatch at {:?}: {}", span, e))
                })
            }
            
            Constraint::IsMay { may_type, inner_type, span } => {
                let expected = InferenceType::Application {
                    constructor: TypeId::MAY,
                    args: vec![inner_type],
                };
                self.unify(may_type, expected).map_err(|e| {
                    Error::Codegen(format!("May type mismatch at {:?}: {}", span, e))
                })
            }
            
            Constraint::IsArray { array_type, elem_type, span } => {
                let expected = InferenceType::Array(Box::new(elem_type));
                self.unify(array_type, expected).map_err(|e| {
                    Error::Codegen(format!("Array type mismatch at {:?}: {}", span, e))
                })
            }
            
            Constraint::HasField { object_type, field_name, field_type, span } => {
                // For now, just ensure field type is resolved
                // Full implementation would look up type definitions
                Ok(())
            }
            
            Constraint::HasMethod { object_type, method_name, param_types, return_type, span } => {
                // Similar to HasField, would need type definition lookup
                Ok(())
            }
        }
    }
    
    /// Unify two types
    fn unify(&mut self, t1: InferenceType, t2: InferenceType) -> Result<(), String> {
        let t1 = self.resolve(t1);
        let t2 = self.resolve(t2);
        
        match (t1, t2) {
            // Same variable
            (InferenceType::Variable(v1), InferenceType::Variable(v2)) if v1 == v2 => Ok(()),
            
            // Variable on left
            (InferenceType::Variable(v), t) | (t, InferenceType::Variable(v)) => {
                // Occurs check
                if t.occurs(&v) {
                    return Err(format!("Infinite type: ?{} occurs in {:?}", v.id, t));
                }
                self.substitutions.insert(v, t);
                Ok(())
            }
            
            // Concrete types
            (InferenceType::Concrete(id1), InferenceType::Concrete(id2)) => {
                if id1 == id2 {
                    Ok(())
                } else {
                    Err(format!("Cannot unify {:?} with {:?}", id1, id2))
                }
            }
            
            // Generic applications
            (InferenceType::Application { constructor: c1, args: a1 },
             InferenceType::Application { constructor: c2, args: a2 }) => {
                if c1 != c2 {
                    return Err(format!("Cannot unify {:?} with {:?}", c1, c2));
                }
                if a1.len() != a2.len() {
                    return Err(format!("Arity mismatch: {} vs {}", a1.len(), a2.len()));
                }
                for (arg1, arg2) in a1.into_iter().zip(a2) {
                    self.unify(arg1, arg2)?;
                }
                Ok(())
            }
            
            // Function types
            (InferenceType::Function { params: p1, ret: r1 },
             InferenceType::Function { params: p2, ret: r2 }) => {
                if p1.len() != p2.len() {
                    return Err(format!("Function arity mismatch: {} vs {}", p1.len(), p2.len()));
                }
                for (param1, param2) in p1.into_iter().zip(p2) {
                    self.unify(param1, param2)?;
                }
                self.unify(*r1, *r2)
            }
            
            // Array types
            (InferenceType::Array(e1), InferenceType::Array(e2)) => {
                self.unify(*e1, *e2)
            }
            
            // Tuple types
            (InferenceType::Tuple(e1), InferenceType::Tuple(e2)) => {
                if e1.len() != e2.len() {
                    return Err(format!("Tuple arity mismatch: {} vs {}", e1.len(), e2.len()));
                }
                for (elem1, elem2) in e1.into_iter().zip(e2) {
                    self.unify(elem1, elem2)?;
                }
                Ok(())
            }
            
            // Mismatched types
            (t1, t2) => Err(format!("Cannot unify {} with {}", t1, t2))
        }
    }
    
    /// Resolve a type by following substitutions
    fn resolve(&self, ty: InferenceType) -> InferenceType {
        match ty {
            InferenceType::Variable(v) => {
                if let Some(subst) = self.substitutions.get(&v) {
                    // Follow chain of substitutions
                    self.resolve(subst.clone())
                } else {
                    ty
                }
            }
            
            InferenceType::Application { constructor, args } => {
                InferenceType::Application {
                    constructor,
                    args: args.into_iter().map(|arg| self.resolve(arg)).collect(),
                }
            }
            
            InferenceType::Function { params, ret } => {
                InferenceType::Function {
                    params: params.into_iter().map(|p| self.resolve(p)).collect(),
                    ret: Box::new(self.resolve(*ret)),
                }
            }
            
            InferenceType::Array(elem) => {
                InferenceType::Array(Box::new(self.resolve(*elem)))
            }
            
            InferenceType::Tuple(elems) => {
                InferenceType::Tuple(elems.into_iter().map(|e| self.resolve(e)).collect())
            }
            
            InferenceType::Concrete(_) => ty,
        }
    }
    
    /// Apply substitution to a type
    pub fn substitute(&self, ty: InferenceType) -> InferenceType {
        self.resolve(ty)
    }
}