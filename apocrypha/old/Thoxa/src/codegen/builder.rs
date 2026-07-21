//! QBE IR builder - helpers for emitting QBE instructions

use std::fmt::Write;
use crate::error::Result;

/// QBE value types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QbeType {
    /// Word (32-bit)
    W,
    /// Long (64-bit) 
    L,
    /// Single precision float
    S,
    /// Double precision float
    D,
    /// Byte
    B,
    /// Half word (16-bit)
    H,
}

impl std::fmt::Display for QbeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QbeType::W => write!(f, "w"),
            QbeType::L => write!(f, "l"),
            QbeType::S => write!(f, "s"),
            QbeType::D => write!(f, "d"),
            QbeType::B => write!(f, "b"),
            QbeType::H => write!(f, "h"),
        }
    }
}

/// Builder for QBE IR output
pub struct Builder {
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Whether we're in a function
    in_function: bool,
    /// Counter for generating unique labels
    label_counter: u32,
    /// Data section for string literals and other constants
    data_section: String,
    /// Counter for generating unique data labels
    data_counter: u32,
    /// Whether the current basic block has been terminated
    terminated: bool,
}

impl Builder {
    /// Create a new QBE builder
    pub fn new() -> Self {
        Builder {
            output: String::new(),
            indent: 0,
            in_function: false,
            label_counter: 0,
            data_section: String::new(),
            data_counter: 0,
            terminated: false,
        }
    }
    
    /// Get the generated output
    pub fn finish(self) -> String {
        // Combine data section and code section
        if self.data_section.is_empty() {
            self.output
        } else {
            format!("# Data section\n{}\n# Code section\n{}", self.data_section, self.output)
        }
    }
    
    /// Get a copy of the current output (for debugging)
    pub fn current_output(&self) -> String {
        self.output.clone()
    }
    
    /// Write a line with current indentation
    fn write_line(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        writeln!(&mut self.output, "{}", line).unwrap();
    }
    
    /// Write a comment to the output
    pub fn comment(&mut self, text: &str) {
        self.write_line(&format!("# {}", text));
    }
    
    /// Write a raw line (no indentation)
    fn write_raw(&mut self, line: &str) {
        writeln!(&mut self.output, "{}", line).unwrap();
    }
    
    /// Check if the current basic block has been terminated
    pub fn is_terminated(&self) -> bool {
        self.terminated
    }
    
    /// Reset termination status (called when starting a new basic block)
    pub fn reset_termination(&mut self) {
        self.terminated = false;
    }
    
    /// Start a function definition
    pub fn function_start(&mut self, name: &str, params: &[(&str, QbeType)], ret_type: QbeType, export: bool) -> Result<()> {
        if self.in_function {
            return Err(crate::error::Error::Codegen(
                "Cannot start function inside another function".to_string()
            ));
        }
        
        // Build parameter list
        let params_str = if params.is_empty() {
            String::new()
        } else {
            params.iter()
                .map(|(name, ty)| format!("{} %{}", ty, name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        
        if export {
            self.write_raw(&format!("export function {} ${}({})", ret_type, name, params_str));
        } else {
            self.write_raw(&format!("function {} ${}({})", ret_type, name, params_str));
        }
        self.write_raw("{");
        
        self.indent = 0;
        self.in_function = true;
        Ok(())
    }
    
    /// End a function definition
    pub fn function_end(&mut self) -> Result<()> {
        if !self.in_function {
            return Err(crate::error::Error::Codegen(
                "Not in a function".to_string()
            ));
        }
        
        self.write_raw("}");
        self.write_raw("");
        self.in_function = false;
        Ok(())
    }
    
    /// Start a new basic block
    pub fn label(&mut self, name: &str) {
        self.write_raw(&format!("@{}", name));
        self.indent = 1;
        // Reset termination status when starting a new basic block
        self.terminated = false;
    }
    
    /// Generate a unique label name
    pub fn gen_label(&mut self, prefix: &str) -> String {
        let label = format!("{}.{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }
    
    /// Emit a return instruction
    pub fn ret(&mut self, value: Option<&str>) {
        if !self.terminated {
            if let Some(val) = value {
                self.write_line(&format!("ret {}", val));
            } else {
                self.write_line("ret");
            }
            self.terminated = true;
        }
    }
    
    /// Emit an add instruction
    pub fn add(&mut self, dst: &str, ty: QbeType, left: &str, right: &str) {
        self.write_line(&format!("{} ={} add {}, {}", dst, ty, left, right));
    }
    
    /// Emit a sub instruction
    pub fn sub(&mut self, dst: &str, ty: QbeType, left: &str, right: &str) {
        self.write_line(&format!("{} ={} sub {}, {}", dst, ty, left, right));
    }
    
    /// Emit a mul instruction
    pub fn mul(&mut self, dst: &str, ty: QbeType, left: &str, right: &str) {
        self.write_line(&format!("{} ={} mul {}, {}", dst, ty, left, right));
    }
    
    /// Emit a div instruction
    pub fn div(&mut self, dst: &str, ty: QbeType, left: &str, right: &str) {
        self.write_line(&format!("{} ={} div {}, {}", dst, ty, left, right));
    }
    
    /// Emit a rem (modulo) instruction
    pub fn rem(&mut self, dst: &str, ty: QbeType, left: &str, right: &str) {
        self.write_line(&format!("{} ={} rem {}, {}", dst, ty, left, right));
    }
    
    /// Emit a comparison instruction
    pub fn cmp(&mut self, dst: &str, op: &str, ty: QbeType, left: &str, right: &str) {
        // QBE comparison instructions use the form: csltl, csgtl, ceql, etc.
        // For integers: 
        //   - Equality/inequality: ceq, cne (no 's' prefix)
        //   - Ordered comparisons: cslt, csle, csgt, csge (signed)
        // For floats: 'c' + op + type
        // The final character is the type suffix
        
        let instruction = match ty {
            QbeType::W | QbeType::L => {
                let type_suffix = if ty == QbeType::W { "w" } else { "l" };
                // Equality and inequality don't use signed prefix
                if op == "eq" || op == "ne" {
                    format!("c{}{}", op, type_suffix)
                } else {
                    // Other comparisons need signed prefix
                    format!("cs{}{}", op, type_suffix)
                }
            }
            QbeType::S | QbeType::D => {
                // Float/double comparison: no 's' prefix
                let type_suffix = if ty == QbeType::S { "s" } else { "d" };
                format!("c{}{}", op, type_suffix)
            }
            _ => {
                // Default to signed long
                if op == "eq" || op == "ne" {
                    format!("c{}l", op)
                } else {
                    format!("cs{}l", op)
                }
            }
        };
        
        self.write_line(&format!("{} ={} {} {}, {}", dst, QbeType::W, instruction, left, right));
    }
    
    /// Emit a jump instruction
    pub fn jmp(&mut self, label: &str) {
        if !self.terminated {
            self.write_line(&format!("jmp @{}", label));
            self.terminated = true;
        }
    }
    
    /// Emit a conditional jump
    pub fn jnz(&mut self, cond: &str, true_label: &str, false_label: &str) {
        if !self.terminated {
            self.write_line(&format!("jnz {}, @{}, @{}", cond, true_label, false_label));
            self.terminated = true;
        }
    }
    
    /// Emit an alloc instruction (stack allocation)
    pub fn alloc(&mut self, dst: &str, size: usize, align: usize) {
        self.write_line(&format!("{} ={} alloc{} {}", dst, QbeType::L, align, size));
    }
    
    /// Emit a store instruction
    pub fn store(&mut self, ty: QbeType, value: &str, addr: &str) {
        self.write_line(&format!("store{} {}, {}", ty, value, addr));
    }
    
    /// Emit a load instruction
    pub fn load(&mut self, dst: &str, ty: QbeType, addr: &str) {
        // For halfword loads, we need to use word type for the destination and loaduh instruction
        if ty == QbeType::H {
            self.write_line(&format!("{} =w loaduh {}", dst, addr));
        } else {
            self.write_line(&format!("{} ={} load{} {}", dst, ty, ty, addr));
        }
    }
    
    /// Emit a copy instruction
    pub fn copy(&mut self, dst: &str, src: &str) {
        if !self.terminated {
            self.write_line(&format!("{} ={} copy {}", dst, QbeType::L, src));
        }
    }
    
    /// Emit a typed copy instruction
    pub fn copy_typed(&mut self, dst: &str, ty: QbeType, src: &str) {
        if !self.terminated {
            self.write_line(&format!("{} ={} copy {}", dst, ty, src));
        }
    }
    
    /// Emit a call instruction
    pub fn call(&mut self, dst: Option<&str>, ret_ty: QbeType, func: &str, args: &[(&str, QbeType)]) {
        let args_str = args.iter()
            .map(|(arg, ty)| format!("{} {}", ty, arg))
            .collect::<Vec<_>>()
            .join(", ");
        
        if let Some(dst) = dst {
            self.write_line(&format!("{} ={} call ${}({})", dst, ret_ty, func, args_str));
        } else {
            self.write_line(&format!("call ${}({})", func, args_str));
        }
    }
    
    /// Emit a void function call (no return value)
    pub fn call_void(&mut self, func: &str, args: Vec<(String, &str)>) {
        let args_str = args.iter()
            .map(|(arg, ty)| format!("{} {}", ty, arg))
            .collect::<Vec<_>>()
            .join(", ");
        
        self.write_line(&format!("call ${}({})", func, args_str));
    }
    
    /// Emit an indirect call instruction (call through function pointer)
    pub fn call_indirect(&mut self, dst: Option<&str>, ret_ty: QbeType, func_ptr: &str, args: &[(&str, QbeType)]) {
        let args_str = args.iter()
            .map(|(arg, ty)| format!("{} {}", ty, arg))
            .collect::<Vec<_>>()
            .join(", ");
        
        if let Some(dst) = dst {
            self.write_line(&format!("{} ={} call {}({})", dst, ret_ty, func_ptr, args_str));
        } else {
            self.write_line(&format!("call {}({})", func_ptr, args_str));
        }
    }
    
    /// Emit a raw QBE instruction
    pub fn instruction(&mut self, instr: String) {
        self.write_line(&instr);
    }
    
    /// Emit a phi instruction
    pub fn phi(&mut self, dst: &str, ty: QbeType, branches: &[(&str, &str)]) {
        let branches_str = branches.iter()
            .map(|(label, value)| format!("@{} {}", label, value))
            .collect::<Vec<_>>()
            .join(", ");
        
        self.write_line(&format!("{} ={} phi {}", dst, ty, branches_str));
    }
    
    /// Add a string literal to the data section
    pub fn add_string_literal(&mut self, value: &str) -> String {
        let label = format!("str.{}", self.data_counter);
        self.data_counter += 1;
        
        // Escape the string and convert to QBE format
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        
        // QBE data format: data $label = { b "string", b 0 }
        // We need to include a null terminator for C compatibility
        writeln!(&mut self.data_section, 
            "data ${} = {{ b \"{}\", b 0 }}", 
            label, escaped
        ).unwrap();
        
        format!("${}", label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_function() {
        let mut builder = Builder::new();
        
        builder.function_start("main", &[], QbeType::L, true).unwrap();
        builder.label("start");
        builder.ret(Some("0"));
        builder.function_end().unwrap();
        
        let output = builder.finish();
        assert!(output.contains("export function l $main()"));
        assert!(output.contains("@start"));
        assert!(output.contains("ret 0"));
    }
    
    #[test]
    fn test_arithmetic() {
        let mut builder = Builder::new();
        
        builder.function_start("add", &[], QbeType::L, false).unwrap();
        builder.label("start");
        builder.add("%t1", QbeType::L, "%a", "%b");
        builder.ret(Some("%t1"));
        builder.function_end().unwrap();
        
        let output = builder.finish();
        assert!(output.contains("%t1 =l add %a, %b"));
    }
    
    #[test]
    fn test_control_flow() {
        let mut builder = Builder::new();
        
        builder.function_start("test", &[], QbeType::L, false).unwrap();
        builder.label("start");
        builder.cmp("%t1", "eq", QbeType::L, "%x", "0");
        builder.jnz("%t1", "true", "false");
        
        builder.label("true");
        builder.ret(Some("1"));
        
        builder.label("false");
        builder.ret(Some("0"));
        
        builder.function_end().unwrap();
        
        let output = builder.finish();
        assert!(output.contains("jnz %t1, @true, @false"));
    }
}