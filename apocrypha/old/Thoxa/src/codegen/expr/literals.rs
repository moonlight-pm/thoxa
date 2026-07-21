//! Literal expression code generation

use crate::codegen::{CodeGen, types::TypeId};
use crate::error::Result;
use crate::lexer::{IntSuffix, FloatSuffix};
use super::ExprResult;

impl CodeGen {
    /// Generate code for an integer literal
    pub(super) fn gen_int_literal(&mut self, value: i64, suffix: &IntSuffix) -> Result<ExprResult> {
        // Determine the type based on suffix
        let type_id = match suffix {
            IntSuffix::None | IntSuffix::I64 => TypeId::INT64,
            IntSuffix::I8 => TypeId::INT8,
            IntSuffix::I16 => TypeId::INT16,
            IntSuffix::I32 => TypeId::INT32,
            IntSuffix::U8 => TypeId::UINT8,
            IntSuffix::U16 => TypeId::UINT16,
            IntSuffix::U32 => TypeId::UINT32,
            IntSuffix::U64 => TypeId::UINT64,
        };
        
        // In Thoxa, everything is a 16-byte value (type pointer + data)
        // For now, we'll generate a simple temporary holding the integer value
        // Later we'll need to pack this into the proper 16-byte structure
        
        let temp = self.symbol_table.gen_temp();
        
        // For now, just copy the literal value
        // TODO: Pack into 16-byte value with type pointer
        self.qbe.copy(&temp, &format!("{}", value));
        
        Ok(ExprResult {
            temp,
            ty: type_id,
        })
    }
    
    /// Generate code for a boolean literal
    pub(super) fn gen_bool_literal(&mut self, value: bool) -> Result<ExprResult> {
        let temp = self.symbol_table.gen_temp();
        
        // Booleans are represented as 0 or 1
        let bool_value = if value { "1" } else { "0" };
        self.qbe.copy(&temp, bool_value);
        
        Ok(ExprResult {
            temp,
            ty: TypeId::BOOL,
        })
    }
    
    /// Generate code for a void literal
    pub(super) fn gen_void_literal(&mut self) -> Result<ExprResult> {
        let temp = self.symbol_table.gen_temp();
        
        // Void has no data, just use 0
        self.qbe.copy(&temp, "0");
        
        Ok(ExprResult {
            temp,
            ty: TypeId::VOID,
        })
    }
    
    /// Generate code for a float literal
    pub(super) fn gen_float_literal(&mut self, value: f64, suffix: &FloatSuffix) -> Result<ExprResult> {
        // Determine the type based on suffix
        let type_id = match suffix {
            FloatSuffix::None | FloatSuffix::F64 => TypeId::FLOAT64,
            FloatSuffix::F32 => TypeId::FLOAT32,
        };
        
        let temp = self.symbol_table.gen_temp();
        
        // QBE syntax for float literals
        // For single precision, use 's_' prefix, for double use 'd_'
        let (qbe_value, qbe_type) = match suffix {
            FloatSuffix::F32 => (format!("s_{}", value as f32), crate::codegen::builder::QbeType::S),
            _ => (format!("d_{}", value), crate::codegen::builder::QbeType::D),
        };
        
        // TODO: Pack into 16-byte value with type pointer
        self.qbe.copy_typed(&temp, qbe_type, &qbe_value);
        
        Ok(ExprResult {
            temp,
            ty: type_id,
        })
    }
    
    /// Generate code for a string literal
    pub(super) fn gen_string_literal(&mut self, value: &str) -> Result<ExprResult> {
        // Add the string to the data section and get its label
        let string_label = self.qbe.add_string_literal(value);
        
        // Create a ThoxaString object at runtime
        let string_obj = self.symbol_table.gen_temp();
        
        // Call thoxa_string_new to create a proper string object
        self.qbe.call(
            Some(&string_obj),
            crate::codegen::builder::QbeType::L,
            "thoxa_string_new",
            &[(&string_label, crate::codegen::builder::QbeType::L)]
        );
        
        Ok(ExprResult {
            temp: string_obj,
            ty: TypeId::STRING,
        })
    }
    
    /// Generate code for a character literal
    pub(super) fn gen_char_literal(&mut self, value: char) -> Result<ExprResult> {
        let temp = self.symbol_table.gen_temp();
        
        // Characters are stored as u32 (Unicode code point)
        let char_value = value as u32;
        self.qbe.copy(&temp, &format!("{}", char_value));
        
        Ok(ExprResult {
            temp,
            ty: TypeId::CHAR,  // Use CHAR type instead of UINT32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use crate::lexer::Span;
    
    fn make_span() -> Span {
        Span { start: 0, end: 0, line: 1, column: 1 }
    }
    
    #[test]
    fn test_int_literal() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::IntLiteral {
            value: 42,
            suffix: IntSuffix::None,
            span: make_span(),
        };
        
        // Need to be in a function to generate code
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64); // Default int type
        assert!(result.temp.starts_with("%t."));
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy"));
        assert!(output.contains("42"));
    }
    
    #[test]
    fn test_bool_literal() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::BoolLiteral {
            value: true,
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::BOOL);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy"));
        assert!(output.contains("1")); // true = 1
    }
    
    #[test]
    fn test_void_literal() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::VoidLiteral {
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy"));
        assert!(output.contains("0")); // void = 0
    }
    
    #[test]
    fn test_float_literal() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::FloatLiteral {
            value: 3.14,
            suffix: FloatSuffix::None,
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::FLOAT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy"));
        assert!(output.contains("d_3.14"));
    }
    
    #[test]
    fn test_char_literal() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::CharLiteral {
            value: 'A',
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::CHAR);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy"));
        assert!(output.contains("65")); // 'A' = 65
    }
}