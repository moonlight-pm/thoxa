use super::types::InferenceType;
use crate::lexer::Span;
use std::fmt;

/// Represents a constraint between types
#[derive(Debug, Clone)]
pub enum Constraint {
    /// Two types must be equal
    Equal(InferenceType, InferenceType, Span),
    
    /// Type must have a field with given name and type
    HasField {
        object_type: InferenceType,
        field_name: String,
        field_type: InferenceType,
        span: Span,
    },
    
    /// Type must have a method with given signature
    HasMethod {
        object_type: InferenceType,
        method_name: String,
        param_types: Vec<InferenceType>,
        return_type: InferenceType,
        span: Span,
    },
    
    /// Type must support numeric operations
    IsNumeric(InferenceType, Span),
    
    /// Type must be a Trial with specific Pass/Fail types
    IsTrial {
        trial_type: InferenceType,
        pass_type: InferenceType,
        fail_type: InferenceType,
        span: Span,
    },
    
    /// Type must be a May with specific inner type
    IsMay {
        may_type: InferenceType,
        inner_type: InferenceType,
        span: Span,
    },
    
    /// Type must support comparison operations
    IsComparable(InferenceType, Span),
    
    /// Type must be an array with specific element type
    IsArray {
        array_type: InferenceType,
        elem_type: InferenceType,
        span: Span,
    },
}

/// Collection of constraints to be solved
#[derive(Debug, Default)]
pub struct ConstraintSet {
    pub constraints: Vec<Constraint>,
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }
    
    pub fn add(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }
    
    pub fn add_equal(&mut self, t1: InferenceType, t2: InferenceType, span: Span) {
        self.add(Constraint::Equal(t1, t2, span));
    }
    
    pub fn add_numeric(&mut self, ty: InferenceType, span: Span) {
        self.add(Constraint::IsNumeric(ty, span));
    }
    
    pub fn add_comparable(&mut self, ty: InferenceType, span: Span) {
        self.add(Constraint::IsComparable(ty, span));
    }
    
    pub fn add_field(&mut self, object: InferenceType, field: String, field_type: InferenceType, span: Span) {
        self.add(Constraint::HasField {
            object_type: object,
            field_name: field,
            field_type,
            span,
        });
    }
    
    pub fn add_method(&mut self, 
        object: InferenceType, 
        method: String, 
        params: Vec<InferenceType>, 
        ret: InferenceType, 
        span: Span
    ) {
        self.add(Constraint::HasMethod {
            object_type: object,
            method_name: method,
            param_types: params,
            return_type: ret,
            span,
        });
    }
    
    pub fn add_trial(&mut self, trial: InferenceType, pass: InferenceType, fail: InferenceType, span: Span) {
        self.add(Constraint::IsTrial {
            trial_type: trial,
            pass_type: pass,
            fail_type: fail,
            span,
        });
    }
    
    pub fn add_may(&mut self, may: InferenceType, inner: InferenceType, span: Span) {
        self.add(Constraint::IsMay {
            may_type: may,
            inner_type: inner,
            span,
        });
    }
    
    pub fn add_array(&mut self, array: InferenceType, elem: InferenceType, span: Span) {
        self.add(Constraint::IsArray {
            array_type: array,
            elem_type: elem,
            span,
        });
    }
    
    /// Merge another constraint set into this one
    pub fn merge(&mut self, other: ConstraintSet) {
        self.constraints.extend(other.constraints);
    }
    
    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.constraints.len()
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constraint::Equal(t1, t2, _) => {
                write!(f, "{} = {}", t1, t2)
            }
            Constraint::HasField { object_type, field_name, field_type, .. } => {
                write!(f, "{}.{} : {}", object_type, field_name, field_type)
            }
            Constraint::HasMethod { object_type, method_name, param_types, return_type, .. } => {
                write!(f, "{}.{}(", object_type, method_name)?;
                for (i, param) in param_types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") -> {}", return_type)
            }
            Constraint::IsNumeric(ty, _) => {
                write!(f, "Numeric({})", ty)
            }
            Constraint::IsTrial { trial_type, pass_type, fail_type, .. } => {
                write!(f, "{} = Trial<{}, {}>", trial_type, pass_type, fail_type)
            }
            Constraint::IsMay { may_type, inner_type, .. } => {
                write!(f, "{} = May<{}>", may_type, inner_type)
            }
            Constraint::IsComparable(ty, _) => {
                write!(f, "Comparable({})", ty)
            }
            Constraint::IsArray { array_type, elem_type, .. } => {
                write!(f, "{} = [{}]", array_type, elem_type)
            }
        }
    }
}

impl Constraint {
    pub fn span(&self) -> &Span {
        match self {
            Constraint::Equal(_, _, span) |
            Constraint::HasField { span, .. } |
            Constraint::HasMethod { span, .. } |
            Constraint::IsNumeric(_, span) |
            Constraint::IsTrial { span, .. } |
            Constraint::IsMay { span, .. } |
            Constraint::IsComparable(_, span) |
            Constraint::IsArray { span, .. } => span,
        }
    }
}