pub mod basic_values;
pub mod control_flow;
pub mod field_reference;
pub mod main;
pub mod system_io;
pub mod templates_arrays;
pub mod type_operations;

// Re-export the main translate_expr function
pub use main::translate_expr;