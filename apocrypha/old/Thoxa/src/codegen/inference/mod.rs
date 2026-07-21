pub mod types;
pub mod constraints;
pub mod generate;
pub mod unify;
pub mod substitute;

use crate::ast::Module;
use crate::codegen::CodeGen;
use crate::error::Error;

pub use types::{InferenceType, TypeVar, TypeScheme};
pub use constraints::{Constraint, ConstraintSet};
pub use generate::ConstraintGenerator;
pub use unify::{Unifier, Substitution};
pub use substitute::TypeSubstitutor;

/// Main entry point for type inference
pub struct TypeInferenceEngine {
    var_counter: u32,
}

impl TypeInferenceEngine {
    pub fn new() -> Self {
        Self {
            var_counter: 0,
        }
    }
    
    /// Perform type inference on a module
    pub fn infer_module(&mut self, module: &Module, codegen: &mut CodeGen) -> Result<Substitution, Error> {
        // Phase 1: Generate constraints
        let mut generator = ConstraintGenerator::new(codegen, &mut self.var_counter);
        let constraints = generator.generate_module(module)?;
        
        // Phase 2: Solve constraints
        let mut unifier = Unifier::new();
        let substitution = unifier.solve(constraints)?;
        
        // Phase 3: Check for ambiguous types
        self.check_ambiguous_types(&substitution)?;
        
        Ok(substitution)
    }
    
    /// Apply inferred types to a module
    pub fn apply_inference(&self, module: &mut Module, substitution: &Substitution) -> Result<(), Error> {
        let substitutor = TypeSubstitutor::new(substitution.clone());
        substitutor.substitute_module(module);
        Ok(())
    }
    
    fn check_ambiguous_types(&self, substitution: &Substitution) -> Result<(), Error> {
        // Check if any type variables remain unresolved
        for (var, ty) in substitution.iter() {
            if let InferenceType::Variable(v) = ty {
                if v == var {
                    return Err(Error::Codegen(format!(
                        "Ambiguous type variable: {:?}. Consider adding type annotations.",
                        var
                    )));
                }
            }
        }
        Ok(())
    }
    
    pub fn fresh_type_var(&mut self) -> TypeVar {
        let var = TypeVar {
            id: self.var_counter,
            name: None,
            level: 0,
        };
        self.var_counter += 1;
        var
    }
}