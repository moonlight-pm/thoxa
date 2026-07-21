use cranelift::prelude::*;

/// Get the Cranelift type for values in Thoxa
/// All values are 64-bit tagged pointers
pub fn value_type() -> cranelift::prelude::Type {
    types::I64
}

/// Tag an integer value
pub fn tag_int_value(builder: &mut FunctionBuilder, value: Value) -> Value {
    // Shift left by 3 and set tag bits to 001
    let shifted = builder.ins().ishl_imm(value, 3);
    builder.ins().bor_imm(shifted, 0b001)
}

/// Untag an integer value
pub fn untag_int_value(builder: &mut FunctionBuilder, tagged: Value) -> Value {
    // Arithmetic shift right by 3 to preserve sign
    builder.ins().sshr_imm(tagged, 3)
}

/// Check if a value is a tagged integer
pub fn is_tagged_int(builder: &mut FunctionBuilder, value: Value) -> Value {
    // Check if low 3 bits are 001
    let masked = builder.ins().band_imm(value, 0b111);
    builder.ins().icmp_imm(IntCC::Equal, masked, 0b001)
}

/// Tag a boolean value
pub fn tag_bool_value(builder: &mut FunctionBuilder, value: Value) -> Value {
    // true = 0x6, false = 0x2
    let zero = builder.ins().iconst(types::I64, 0);
    let is_true = builder.ins().icmp(IntCC::NotEqual, value, zero);
    let true_val = builder.ins().iconst(types::I64, 0x6);
    let false_val = builder.ins().iconst(types::I64, 0x2);
    builder.ins().select(is_true, true_val, false_val)
}

/// Check if a value is a pointer (low 3 bits are 0)
pub fn is_tagged_ptr(builder: &mut FunctionBuilder, value: Value) -> Value {
    let masked = builder.ins().band_imm(value, 0x7);
    builder.ins().icmp_imm(IntCC::Equal, masked, 0)
}

/// Extract the value part from a tagged value
pub fn extract_value(builder: &mut FunctionBuilder, tagged: Value) -> Value {
    // Check if it's a tagged integer
    let is_int = is_tagged_int(builder, tagged);
    
    // Create blocks for the conditional
    let int_block = builder.create_block();
    let ptr_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);
    
    builder.ins().brif(is_int, int_block, &[], ptr_block, &[]);
    
    // Integer path - untag it
    builder.seal_block(int_block);
    builder.switch_to_block(int_block);
    let untagged = untag_int_value(builder, tagged);
    builder.ins().jump(merge_block, &[untagged]);
    
    // Pointer path - return as-is (it's already a pointer)
    builder.seal_block(ptr_block);
    builder.switch_to_block(ptr_block);
    builder.ins().jump(merge_block, &[tagged]);
    
    // Merge
    builder.seal_block(merge_block);
    builder.switch_to_block(merge_block);
    builder.block_params(merge_block)[0]
}