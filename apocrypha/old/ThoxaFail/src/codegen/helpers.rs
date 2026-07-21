use crate::ast::Expr;
use crate::runtime::BuiltinTypeId;
use cranelift::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// Information about a module type for code generation
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub fields: Vec<(String, bool, Expr)>, // (field_name, is_exported, init_expr)
    pub field_offsets: HashMap<String, usize>,
    pub size: usize,
}

impl ModuleInfo {
    pub fn new(name: String) -> Self {
        Self {
            name,
            fields: Vec::new(),
            field_offsets: HashMap::new(),
            size: 0,
        }
    }
    
    pub fn add_field(&mut self, name: String, exported: bool, init_expr: Expr) {
        let offset = self.fields.len();
        self.fields.push((name.clone(), exported, init_expr));
        self.field_offsets.insert(name, offset);
        self.size = self.fields.len() * 8; // Each field is 8 bytes (i64)
    }
    
    pub fn get_field_offset(&self, field: &str) -> Option<usize> {
        self.field_offsets.get(field).copied()
    }
}

/// Module type registry for tracking module definitions
#[derive(Debug, Clone)]
pub struct ModuleRegistry {
    pub modules: HashMap<String, ModuleInfo>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }
    
    pub fn register_module(&mut self, name: String, info: ModuleInfo) {
        self.modules.insert(name, info);
    }
    
    pub fn get_type(&self, name: &str) -> Option<&ModuleInfo> {
        self.modules.get(name)
    }
    
    // Alias for get_type to maintain compatibility
    pub fn get_module(&self, name: &str) -> Option<&ModuleInfo> {
        self.get_type(name)
    }
}

// Thread-local storage for loop control blocks
thread_local! {
    pub static LOOP_EXIT_BLOCKS: RefCell<Vec<Block>> = RefCell::new(Vec::new());
    pub static LOOP_CONTINUE_BLOCKS: RefCell<Vec<Block>> = RefCell::new(Vec::new());
    pub static LOOP_VARIABLE_INFO: RefCell<Vec<Vec<(String, Variable)>>> = RefCell::new(Vec::new());
}

/// Convert a Thoxa Type to a Cranelift Type
/// NOTE: This is not currently used but shows what proper type mapping would look like
#[allow(dead_code)]
pub fn thoxa_type_to_cranelift(ty: &crate::types::Type) -> types::Type {
    use crate::types::Type;
    match ty {
        Type::Int { signed: _, bits: 8 } => types::I8,
        Type::Int { signed: _, bits: 16 } => types::I16,
        Type::Int { signed: _, bits: 32 } => types::I32,
        Type::Int { signed: _, bits: 64 } => types::I64,
        Type::Any => types::I64, // Any is represented as a pointer to tagged value
        // All other types map to I64 for now (pointers, etc.)
        _ => types::I64,
    }
}

/// Create an Any value from an immediate value
pub fn create_any_value(builder: &mut FunctionBuilder, value: i64, type_id: BuiltinTypeId) -> Value {
    // Allocate space for Any struct (16 bytes: 8 for type tag, 8 for value)
    let slot = builder.create_sized_stack_slot(StackSlotData {
        kind: StackSlotKind::ExplicitSlot,
        size: 16,
    });
    let base_addr = builder.ins().stack_addr(types::I64, slot, 0);
    
    // Store type tag
    let type_tag = builder.ins().iconst(types::I64, type_id as i64);
    builder.ins().store(MemFlags::new(), type_tag, base_addr, 0);
    
    // Store value
    let val = builder.ins().iconst(types::I64, value);
    builder.ins().store(MemFlags::new(), val, base_addr, 8);
    
    base_addr
}

/// Create an Any value from a pointer value
pub fn create_any_value_from_ptr(builder: &mut FunctionBuilder, ptr: Value, type_id: BuiltinTypeId) -> Value {
    // Allocate space for Any struct (16 bytes: 8 for type tag, 8 for value)
    let slot = builder.create_sized_stack_slot(StackSlotData {
        kind: StackSlotKind::ExplicitSlot,
        size: 16,
    });
    let base_addr = builder.ins().stack_addr(types::I64, slot, 0);
    
    // Store type tag
    let type_tag = builder.ins().iconst(types::I64, type_id as i64);
    builder.ins().store(MemFlags::new(), type_tag, base_addr, 0);
    
    // Store pointer
    builder.ins().store(MemFlags::new(), ptr, base_addr, 8);
    
    base_addr
}