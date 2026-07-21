use crate::types::Type;

/// Represents a typed expression in the AST
/// This enum mirrors Expr but with type information at every level
#[derive(Debug, Clone, PartialEq)]
pub enum TypedExpr {
    Number {
        value: i64,
        ty: Type,
    },
    StringLiteral {
        value: String,
        ty: Type,
    },
    Binary {
        left: Box<TypedExpr>,
        op: BinaryOp,
        right: Box<TypedExpr>,
        ty: Type,
    },
    Identifier {
        name: String,
        ty: Type,
    },
    Sequence {
        exprs: Vec<TypedExpr>,
        ty: Type,
    },
    Block {
        exprs: Vec<TypedExpr>,
        ty: Type,
    },
    LetBinding {
        name: String,
        type_expr: Box<Expr>, // Type expressions remain untyped
        value: Box<TypedExpr>,
        mutable: bool,
        ty: Type,
    },
    Assignment {
        name: String,
        value: Box<TypedExpr>,
        ty: Type,
    },
    FieldAccess {
        object: Box<TypedExpr>,
        field: String,
        ty: Type,
    },
    FunctionCall {
        function: Box<TypedExpr>,
        arguments: Vec<TypedExpr>,
        ty: Type,
    },
    Print {
        value: Box<TypedExpr>,
        ty: Type,
    },
    PrintStr {
        value: Box<TypedExpr>,
        ty: Type,
    },
    If {
        condition: Box<TypedExpr>,
        then_expr: Box<TypedExpr>,
        else_expr: Option<Box<TypedExpr>>,
        ty: Type,
    },
    Loop {
        body: Box<TypedExpr>,
        ty: Type,
    },
    Break {
        value: Option<Box<TypedExpr>>,
        ty: Type,
    },
    Continue {
        ty: Type,
    },
    MethodCall {
        object: Box<TypedExpr>,
        method: String,
        arguments: Vec<TypedExpr>,
        ty: Type,
    },
    ArrayLiteral {
        elements: Vec<TypedExpr>,
        ty: Type,
    },
    ArrayIndex {
        array: Box<TypedExpr>,
        index: Box<TypedExpr>,
        ty: Type,
    },
    Conversion {
        expr: Box<TypedExpr>,
        target_type: Type,
        ty: Type,
    },
    Syscall {
        number: Box<TypedExpr>,
        arguments: Vec<TypedExpr>,
        ty: Type,
    },
    Strlen {
        value: Box<TypedExpr>,
        ty: Type,
    },
    Cstr {
        value: Box<TypedExpr>,
        ty: Type,
    },
    IntToString {
        value: Box<TypedExpr>,
        ty: Type,
    },
    Reference {
        expr: Box<TypedExpr>,
        ty: Type,
    },
    TypeGuard {
        expr: Box<TypedExpr>,
        type_expr: Box<Expr>,
        ty: Type,
    },
    
    // Temporary fallback for unimplemented cases
    Untyped {
        expr: Expr,
        ty: Type,
    },
}

/// Old TypedExpr struct - to be removed after migration
#[derive(Debug, Clone, PartialEq)]
pub struct OldTypedExpr {
    pub expr: Expr,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(i64),
    StringLiteral(String),
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    // Identifiers (for referencing defined types and variables)
    Identifier(String),
    // Built-in type construction helpers
    IntType {
        signed: bool,
        bits: u8,
    },
    // Expression sequences: a; b; c evaluates to c
    Sequence(Vec<Expr>),
    // Block expressions: { expr1; expr2; expr3 }
    Block(Vec<Expr>),
    // Let bindings: let name: Type = value; or mut name: Type = value;
    LetBinding {
        name: String,
        type_expr: Box<Expr>,
        value: Box<Expr>,
        mutable: bool,
    },
    // Assignment: name = value
    Assignment {
        name: String,
        value: Box<Expr>,
    },
    // Type definitions: type Name { members... }
    TypeDef {
        name: String,
        members: Vec<TypeMember>,
    },
    // Field access: object.field
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    // Function call: func(arg1, arg2, ...)
    FunctionCall {
        function: Box<Expr>,
        arguments: Vec<Expr>,
    },
    // Method call: object.method(arg1, arg2, ...)
    MethodCall {
        object: Box<Expr>,
        method: String,
        arguments: Vec<Expr>,
    },
    // Reference expressions: &value
    Reference {
        expr: Box<Expr>,
    },
    // Void type for side-effect functions
    Void,
    // Print builtin: @print(expr)
    Print {
        value: Box<Expr>,
    },
    // PrintStr builtin: @printStr(expr)
    PrintStr {
        value: Box<Expr>,
    },
    // Syscall builtin: @syscall(number, arg1, arg2, ...)
    Syscall {
        number: Box<Expr>,
        arguments: Vec<Expr>,
    },
    // Strlen builtin: @strlen(string)
    Strlen {
        value: Box<Expr>,
    },
    // Cstr builtin: @cstr(string) - returns pointer to C-compatible string data
    Cstr {
        value: Box<Expr>,
    },
    // IntToString builtin: @intToString(integer)
    IntToString {
        value: Box<Expr>,
    },
    // If expression: if condition { then_expr } else { else_expr }
    If {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Option<Box<Expr>>,
    },
    // Loop expression: loop { body }
    Loop {
        body: Box<Expr>,
    },
    // Break expression: break [value]
    Break {
        value: Option<Box<Expr>>,
    },
    // Continue expression: continue
    Continue,
    // Import statement: import Name from "path"
    Import {
        imports: ImportSpec,
        path: String,
    },
    // Template literal: `Hello, ${name}!`
    TemplateLiteral {
        parts: Vec<String>,      // String segments between expressions
        expressions: Vec<Expr>,  // Embedded expressions
    },
    // Array literal: [elem1, elem2, ...]
    ArrayLiteral {
        elements: Vec<Expr>,
    },
    // Array indexing: array[index]
    ArrayIndex {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    // Type conversion: expr as Type
    Conversion {
        expr: Box<Expr>,
        target_type: Box<Expr>,
    },
    // Conversion check: expr can Type
    CanConvert {
        expr: Box<Expr>,
        target_type: Box<Expr>,
    },
    // Type guard: expr is Type
    TypeGuard {
        expr: Box<Expr>,
        type_expr: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSpec {
    // import Name from "path" - imports default instance as Name
    Default(String),
    // import Type, instance from "path" - imports type and instance
    TypeAndInstance {
        type_name: String,
        instance_name: String,
    },
    // import { name1, name2 } from "path" - destructured imports
    Destructured(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeMember {
    // Let binding within a type: [export] let name: Type = value;
    LetBinding {
        name: String,
        type_expr: Expr,
        value: Expr,
        exported: bool,
    },
    // Function definition within a type: [export] fn name(params) -> RetType { body }
    FunctionDef {
        name: String,
        params: Vec<Parameter>,
        return_type: Expr,
        body: Expr,
        exported: bool,
    },
    // Conversion definition: as TypeName { body }
    Conversion {
        target_type: Expr,
        body: Expr,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_expr: Expr,
    pub variadic: bool,  // true for params like "args...: Type"
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    // Comparison operators
    Eq,     // ==
    NotEq,  // !=
    Less,   // <
    Greater, // >
    LessEq, // <=
    GreaterEq, // >=
}

impl TypedExpr {
    /// Get the type of this expression
    pub fn get_type(&self) -> &Type {
        match self {
            TypedExpr::Number { ty, .. } => ty,
            TypedExpr::StringLiteral { ty, .. } => ty,
            TypedExpr::Binary { ty, .. } => ty,
            TypedExpr::Identifier { ty, .. } => ty,
            TypedExpr::Sequence { ty, .. } => ty,
            TypedExpr::Block { ty, .. } => ty,
            TypedExpr::LetBinding { ty, .. } => ty,
            TypedExpr::Assignment { ty, .. } => ty,
            TypedExpr::FieldAccess { ty, .. } => ty,
            TypedExpr::FunctionCall { ty, .. } => ty,
            TypedExpr::Print { ty, .. } => ty,
            TypedExpr::PrintStr { ty, .. } => ty,
            TypedExpr::If { ty, .. } => ty,
            TypedExpr::Loop { ty, .. } => ty,
            TypedExpr::Break { ty, .. } => ty,
            TypedExpr::Continue { ty, .. } => ty,
            TypedExpr::MethodCall { ty, .. } => ty,
            TypedExpr::ArrayLiteral { ty, .. } => ty,
            TypedExpr::ArrayIndex { ty, .. } => ty,
            TypedExpr::Conversion { ty, .. } => ty,
            TypedExpr::Syscall { ty, .. } => ty,
            TypedExpr::Strlen { ty, .. } => ty,
            TypedExpr::Cstr { ty, .. } => ty,
            TypedExpr::IntToString { ty, .. } => ty,
            TypedExpr::Reference { ty, .. } => ty,
            TypedExpr::TypeGuard { ty, .. } => ty,
            TypedExpr::Untyped { ty, .. } => ty,
        }
    }
}

impl OldTypedExpr {
    /// Create a new typed expression
    pub fn new(expr: Expr, ty: Type) -> Self {
        OldTypedExpr { expr, ty }
    }
    
    /// Create a typed expression with unknown type
    pub fn unknown(expr: Expr) -> Self {
        OldTypedExpr { expr, ty: Type::Unknown }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_let_binding_creation() {
        let let_binding = Expr::LetBinding {
            name: "x".to_string(),
            type_expr: Box::new(Expr::Identifier("Int64".to_string())),
            value: Box::new(Expr::Number(42)),
            mutable: false,
        };
        
        // Test that it can be created and cloned
        let cloned = let_binding.clone();
        assert_eq!(let_binding, cloned);
    }
    
    #[test]
    fn test_type_def_creation() {
        let type_def = Expr::TypeDef {
            name: "Point".to_string(),
            members: vec![
                TypeMember::LetBinding {
                    name: "x".to_string(),
                    type_expr: Expr::Identifier("I64".to_string()),
                    value: Expr::Number(0),
                    exported: true,
                },
            ],
        };
        
        // Test Debug output contains expected elements
        let debug_str = format!("{:?}", type_def);
        assert!(debug_str.contains("Point"));
        assert!(debug_str.contains("exported: true"));
    }
    
    #[test]
    fn test_field_access_creation() {
        let field_access = Expr::FieldAccess {
            object: Box::new(Expr::Identifier("p".to_string())),
            field: "x".to_string(),
        };
        
        assert_eq!(field_access, field_access.clone());
    }
    
    #[test]
    fn test_reference_creation() {
        let reference = Expr::Reference {
            expr: Box::new(Expr::Identifier("value".to_string())),
        };
        
        assert_eq!(reference, reference.clone());
    }
    
    #[test]
    fn test_void_type() {
        let void = Expr::Void;
        assert_eq!(void, Expr::Void);
        
        // Test Debug output
        let debug_str = format!("{:?}", void);
        assert_eq!(debug_str, "Void");
    }
    
    #[test]
    fn test_type_member_variants() {
        let let_member = TypeMember::LetBinding {
            name: "x".to_string(),
            type_expr: Expr::Identifier("I64".to_string()),
            value: Expr::Number(42),
            exported: false,
        };
        
        let fn_member = TypeMember::FunctionDef {
            name: "add".to_string(),
            params: vec![
                Parameter {
                    name: "a".to_string(),
                    type_expr: Expr::Identifier("I64".to_string()),
                    variadic: false,
                },
            ],
            return_type: Expr::Identifier("I64".to_string()),
            body: Expr::Number(0),
            exported: true,
        };
        
        let conv_member = TypeMember::Conversion {
            target_type: Expr::Identifier("String".to_string()),
            body: Expr::StringLiteral("converted".to_string()),
        };
        
        // Test that different variants are not equal
        assert_ne!(
            format!("{:?}", let_member),
            format!("{:?}", fn_member)
        );
        assert_ne!(
            format!("{:?}", fn_member),
            format!("{:?}", conv_member)
        );
    }
    
    #[test]
    fn test_if_expression_creation() {
        let if_expr = Expr::If {
            condition: Box::new(Expr::Binary {
                left: Box::new(Expr::Identifier("x".to_string())),
                op: BinaryOp::Greater,
                right: Box::new(Expr::Number(5)),
            }),
            then_expr: Box::new(Expr::Number(10)),
            else_expr: Some(Box::new(Expr::Number(20))),
        };
        
        // Test that it can be created and cloned
        let cloned = if_expr.clone();
        assert_eq!(if_expr, cloned);
    }
    
    #[test]
    fn test_comparison_operators() {
        let ops = vec![
            BinaryOp::Eq,
            BinaryOp::NotEq,
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessEq,
            BinaryOp::GreaterEq,
        ];
        
        // Test that all operators are distinct
        for (i, op1) in ops.iter().enumerate() {
            for (j, op2) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(op1, op2);
                } else {
                    assert_ne!(op1, op2);
                }
            }
        }
    }
    
    #[test]
    fn test_loop_expression_creation() {
        let loop_expr = Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Number(42),
                Expr::Break { value: None },
            ])),
        };
        
        // Test that it can be created and cloned
        let cloned = loop_expr.clone();
        assert_eq!(loop_expr, cloned);
    }
    
    #[test]
    fn test_break_expression_creation() {
        // Break without value
        let break_expr = Expr::Break { value: None };
        assert_eq!(break_expr, Expr::Break { value: None });
        
        // Break with value
        let break_with_val = Expr::Break {
            value: Some(Box::new(Expr::Number(42))),
        };
        
        // Test cloning
        let cloned = break_with_val.clone();
        assert_eq!(break_with_val, cloned);
    }
    
    #[test]
    fn test_continue_expression_creation() {
        let continue_expr = Expr::Continue;
        assert_eq!(continue_expr, Expr::Continue);
        
        // Test Debug output
        let debug_str = format!("{:?}", continue_expr);
        assert_eq!(debug_str, "Continue");
    }
    
    #[test]
    fn test_typed_expr_creation() {
        // Test creating a typed expression
        let typed_expr = TypedExpr::Number {
            value: 42,
            ty: Type::int(64),
        };
        
        assert_eq!(typed_expr.get_type(), &Type::int(64));
        
        // Test string literal
        let string_expr = TypedExpr::StringLiteral {
            value: "hello".to_string(),
            ty: Type::String,
        };
        assert_eq!(string_expr.get_type(), &Type::String);
    }
    
    #[test]
    fn test_import_expression_creation() {
        // Default import
        let import1 = Expr::Import {
            imports: ImportSpec::Default("Math".to_string()),
            path: "./math".to_string(),
        };
        
        // Type and instance import
        let import2 = Expr::Import {
            imports: ImportSpec::TypeAndInstance {
                type_name: "Math".to_string(),
                instance_name: "math".to_string(),
            },
            path: "./math".to_string(),
        };
        
        // Destructured import
        let import3 = Expr::Import {
            imports: ImportSpec::Destructured(vec!["add".to_string(), "pi".to_string()]),
            path: "./math".to_string(),
        };
        
        // Test cloning
        assert_eq!(import1.clone(), import1);
        assert_eq!(import2.clone(), import2);
        assert_eq!(import3.clone(), import3);
    }
}