use crate::lexer::{Span, IntSuffix, FloatSuffix};

// AST nodes for the Thoxa language

/// Segment of an interpolated string
#[derive(Debug, Clone, PartialEq)]
pub enum StringSegment {
    /// Plain text segment
    Text(String),
    /// Interpolated expression {expr}
    Interpolation(Box<Expr>),
}

/// Root of the AST - a module/file
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
    pub span: Span,
}

/// Import statement
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub items: Vec<ImportItem>,
    pub from: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportItem {
    Named(String),
    Renamed { name: String, as_name: String },
}

/// Top-level items in a module
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(Function),
    Type(TypeDef),
    Interface(Interface),
    Unit(UnitDef),
    Union(UnionDef),
}

/// Function definition
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub type_params: Vec<String>,  // Generic type parameters
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
    pub is_export: bool,
    pub span: Span,
}

/// Function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub is_mut: bool,
    pub span: Span,
}

/// Type definition
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Block,
    pub is_export: bool,
    pub span: Span,
}

/// Interface definition
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub name: String,
    pub methods: Vec<InterfaceMethod>,
    pub is_export: bool,
    pub span: Span,
}

/// Unit definition (user-defined data type)
#[derive(Debug, Clone, PartialEq)]
pub struct UnitDef {
    pub name: String,
    pub type_params: Vec<String>,  // Generic type parameters (e.g., ["T", "E"])
    pub constructor_params: Vec<Param>,  // Constructor parameters
    pub body: Vec<Stmt>,  // Body contains statements (fields, methods, init code)
    pub is_export: bool,
    pub span: Span,
}

/// Union type definition (sum type)
#[derive(Debug, Clone, PartialEq)]
pub struct UnionDef {
    pub name: String,
    pub type_params: Vec<String>,  // Generic type parameters (e.g., ["T"] in May<T>)
    pub variants: Vec<Type>,  // The types in the union (e.g., [Has<T>, Not])
    pub is_export: bool,
    pub span: Span,
}


#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub is_export: bool,
    pub span: Span,
}

/// Type expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Named {
        name: String,
        args: Vec<Type>,  // Generic type arguments (e.g., [Int, String] for Map<Int, String>)
        span: Span,
    },
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        span: Span,
    },
    Array {
        elem: Box<Type>,
        span: Span,
    },
    Tuple {
        elems: Vec<Type>,
        span: Span,
    },
    Union {
        variants: Vec<Type>,
        span: Span,
    },
}

/// Statements
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Type>,
        value: Option<Expr>,
        is_mut: bool,
        is_export: bool,
        span: Span,
    },
    Method {
        name: String,
        params: Vec<Param>,
        return_type: Option<Type>,
        body: Block,
        is_export: bool,
        span: Span,
    },
    Expr(Expr),
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        value: Option<Expr>,
        span: Span,
    },
    Continue {
        span: Span,
    },
}

/// Expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Literals
    IntLiteral {
        value: i64,
        suffix: IntSuffix,
        span: Span,
    },
    FloatLiteral {
        value: f64,
        suffix: FloatSuffix,
        span: Span,
    },
    StringLiteral {
        value: String,
        span: Span,
    },
    InterpolatedString {
        segments: Vec<StringSegment>,
        span: Span,
    },
    CharLiteral {
        value: char,
        span: Span,
    },
    BoolLiteral {
        value: bool,
        span: Span,
    },
    VoidLiteral {
        span: Span,
    },
    
    // Variables and paths
    Ident {
        name: String,
        span: Span,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
        span: Span,
    },
    
    // Function/type calls
    Call {
        func: Box<Expr>,
        type_args: Vec<Type>,  // Generic type arguments (e.g., <Int, String>)
        args: Vec<Expr>,
        span: Span,
    },
    
    // Enum variant constructor
    EnumVariant {
        enum_name: Option<String>,  // None for built-in Trial
        variant: String,             // "Pass" or "Fail"
        value: Option<Box<Expr>>,    // The payload
        span: Span,
    },
    
    // Unit construction
    UnitConstruction {
        name: String,
        fields: Vec<(String, Expr)>,  // Field name and value pairs
        span: Span,
    },
    
    // Operators
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    
    // Control flow
    Block(Block),
    If {
        condition: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    Loop {
        body: Block,
        span: Span,
    },
    While {
        condition: Box<Expr>,
        body: Block,
        span: Span,
    },
    ForIn {
        variable: String,
        iterable: Box<Expr>,
        body: Block,
        span: Span,
    },
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    
    // Error propagation
    Try {
        expr: Box<Expr>,
        span: Span,
    },
    
    // Type operations
    As {
        expr: Box<Expr>,
        ty: Type,
        span: Span,
    },
    
    // Arrays
    ArrayLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    
    // Tuples
    Tuple {
        elements: Vec<Expr>,
        span: Span,
    },
    TupleAccess {
        tuple: Box<Expr>,
        index: usize,
        span: Span,
    },
    
    // Assignment
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    /// Get the span of this expression
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLiteral { span, .. } => *span,
            Expr::FloatLiteral { span, .. } => *span,
            Expr::StringLiteral { span, .. } => *span,
            Expr::InterpolatedString { span, .. } => *span,
            Expr::CharLiteral { span, .. } => *span,
            Expr::BoolLiteral { span, .. } => *span,
            Expr::VoidLiteral { span } => *span,
            Expr::Ident { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::EnumVariant { span, .. } => *span,
            Expr::UnitConstruction { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Block(block) => block.span,
            Expr::If { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::ForIn { span, .. } => *span,
            Expr::Match { span, .. } => *span,
            Expr::Try { span, .. } => *span,
            Expr::As { span, .. } => *span,
            Expr::ArrayLiteral { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Tuple { span, .. } => *span,
            Expr::TupleAccess { span, .. } => *span,
            Expr::Assign { span, .. } => *span,
        }
    }
}

/// Block expression
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub expr: Option<Box<Expr>>, // Final expression (no semicolon)
    pub span: Span,
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    
    // Logical
    And,
    Or,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

/// Pattern matching
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    // Literal patterns
    IntLiteral {
        value: i64,
        span: Span,
    },
    StringLiteral {
        value: String,
        span: Span,
    },
    BoolLiteral {
        value: bool,
        span: Span,
    },
    
    // Constructor patterns
    Constructor {
        name: String,
        fields: Option<Vec<PatternField>>,
        span: Span,
    },
    
    // Variable binding
    Ident {
        name: String,
        span: Span,
    },
    
    // Wildcard
    Wildcard {
        span: Span,
    },
    
    // Type patterns
    Type {
        name: String,
        pattern: Box<Pattern>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatternField {
    pub name: String,
    pub pattern: Option<Pattern>, // None means bind to same name
    pub span: Span,
}