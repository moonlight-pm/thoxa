// Core modules
pub mod tagged_values;
pub mod helpers;
pub mod variable;
pub mod func;
pub mod symbols;
pub mod jit;
pub mod aot;
pub mod typed_expr;
pub mod expr;

// Re-export commonly used items
pub use tagged_values::{value_type, extract_value, tag_int_value, tag_bool_value, untag_int_value, is_tagged_int, is_tagged_ptr};
pub use variable::VarEnv;
pub use func::FuncEnv;
pub use helpers::{ModuleInfo, ModuleRegistry, LOOP_EXIT_BLOCKS, LOOP_CONTINUE_BLOCKS, LOOP_VARIABLE_INFO};
pub use typed_expr::translate_typed_expr;
pub use expr::translate_expr;
pub use jit::JITCodegen;

// Re-export type-specific items that might be needed
pub use crate::ast::{Expr, TypedExpr};
pub use crate::types::Type;