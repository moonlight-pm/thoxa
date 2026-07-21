//! Expression code generation

mod arrays;
mod calls;
mod enums;
mod interpolation;
mod literals;
mod operations;
mod try_op;
mod tuples;
mod type_check;
mod units;
mod variables;

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId};
use crate::error::{Error, Result};

/// Result of generating code for an expression
#[derive(Debug, Clone)]
pub struct ExprResult {
    /// The QBE temporary holding the value
    pub temp: String,
    /// The type of the expression
    pub ty: TypeId,
}

impl CodeGen {
    /// Generate code for an expression
    pub fn gen_expr(&mut self, expr: &Expr) -> Result<ExprResult> {
        match expr {
            // Literals
            Expr::IntLiteral { value, suffix, .. } => self.gen_int_literal(*value, suffix),
            Expr::FloatLiteral { value, suffix, .. } => self.gen_float_literal(*value, suffix),
            Expr::StringLiteral { value, .. } => self.gen_string_literal(value),
            Expr::InterpolatedString { segments, .. } => self.gen_interpolated_string(segments),
            Expr::CharLiteral { value, .. } => self.gen_char_literal(*value),
            Expr::BoolLiteral { value, .. } => self.gen_bool_literal(*value),
            Expr::VoidLiteral { .. } => self.gen_void_literal(),
            
            // Binary operations
            Expr::Binary { op, left, right, .. } => self.gen_binary_op(op, left, right),
            
            // Unary operations
            Expr::Unary { op, expr, .. } => self.gen_unary_op(op, expr),
            
            // Variables and identifiers
            Expr::Ident { name, .. } => self.gen_ident(name),
            Expr::FieldAccess { object, field, .. } => self.gen_field_access(object, field),
            
            // Assignment
            Expr::Assign { target, value, .. } => self.gen_assign(target, value),
            
            // Block expressions
            Expr::Block(block) => self.gen_block(block),
            
            // Function calls
            Expr::Call { func, type_args, args, .. } => self.gen_call(func, type_args, args),
            
            // Enum variant constructors
            Expr::EnumVariant { variant, value, .. } => self.gen_enum_variant(variant, value.as_deref()),
            
            // Control flow
            Expr::If { condition, then_block, else_block, .. } => 
                self.gen_if(condition, then_block, else_block.as_ref()),
            Expr::Loop { body, .. } => self.gen_loop(body),
            Expr::While { condition, body, .. } => self.gen_while(condition, body),
            Expr::ForIn { variable, iterable, body, .. } => self.gen_for_in(variable, iterable, body),
            Expr::Match { expr, arms, .. } => self.gen_match(expr, arms),
            
            // Try operator (?)
            Expr::Try { expr, .. } => self.gen_try_op(expr),
            
            // Unit construction
            Expr::UnitConstruction { name, fields, .. } => self.gen_unit_construction(name, fields),
            
            // Arrays
            Expr::ArrayLiteral { elements, .. } => self.gen_array_literal(elements),
            Expr::Index { array, index, .. } => self.gen_array_index(array, index),
            
            // Tuples
            Expr::Tuple { elements, .. } => self.gen_tuple(elements),
            Expr::TupleAccess { tuple, index, .. } => self.gen_tuple_access(tuple, *index),
            
            // For now, other expressions are not implemented
            _ => Err(Error::Codegen(format!("Expression not yet implemented: {:?}", expr))),
        }
    }
}