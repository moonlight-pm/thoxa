pub mod resolution;
pub mod main;
pub mod expressions;
pub mod advanced;

pub use resolution::{
    resolve_type_expr,
    resolve_member_type,
    build_function_type,
    types_compatible,
};

pub use main::type_check;