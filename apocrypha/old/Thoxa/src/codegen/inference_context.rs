//! Inference context to prevent infinite recursion during type inference

use std::collections::HashSet;
use crate::error::{Error, Result};

/// Context for tracking ongoing type inference to prevent infinite recursion
#[derive(Debug, Default)]
pub struct InferenceContext {
    /// Set of function names currently being inferred
    /// We track by name only since we don't know types yet during inference
    inferring: HashSet<String>,
    
    /// Depth of inference for detecting deep recursion
    depth: usize,
    
    /// Maximum allowed inference depth
    max_depth: usize,
}

impl InferenceContext {
    /// Create a new inference context
    pub fn new() -> Self {
        Self {
            inferring: HashSet::new(),
            depth: 0,
            max_depth: 10, // Reasonable limit for nested generic calls
        }
    }
    
    /// Check if we can start inferring types for a function
    pub fn can_infer(&self, func_name: &str) -> bool {
        !self.inferring.contains(func_name) && self.depth < self.max_depth
    }
    
    /// Start inferring types for a function
    /// Returns a guard that must be held during inference
    pub fn start_inference(&mut self, func_name: &str) -> Result<InferenceGuard> {
        if self.inferring.contains(func_name) {
            return Err(Error::Codegen(format!(
                "Recursive type inference detected for function '{}'. \
                 Please provide explicit type arguments.",
                func_name
            )));
        }
        
        if self.depth >= self.max_depth {
            return Err(Error::Codegen(format!(
                "Type inference depth limit exceeded. \
                 Please provide explicit type arguments for nested generic calls."
            )));
        }
        
        self.inferring.insert(func_name.to_string());
        self.depth += 1;
        
        Ok(InferenceGuard {
            func_name: func_name.to_string(),
            context: self as *mut Self,
        })
    }
    
    /// Internal: end inference for a function
    fn end_inference(&mut self, func_name: &str) {
        self.inferring.remove(func_name);
        self.depth = self.depth.saturating_sub(1);
    }
}

/// RAII guard for inference context
/// Automatically cleans up when dropped
pub struct InferenceGuard {
    func_name: String,
    context: *mut InferenceContext,
}

impl std::fmt::Debug for InferenceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InferenceGuard")
            .field("func_name", &self.func_name)
            .finish()
    }
}

impl Drop for InferenceGuard {
    fn drop(&mut self) {
        unsafe {
            // Safe because we know the context outlives the guard
            (*self.context).end_inference(&self.func_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_inference() {
        let mut ctx = InferenceContext::new();
        
        assert!(ctx.can_infer("foo"));
        let _guard = ctx.start_inference("foo").unwrap();
        assert!(!ctx.can_infer("foo")); // Can't infer while already inferring
    }
    
    #[test]
    fn test_recursive_inference_prevented() {
        let mut ctx = InferenceContext::new();
        
        let _guard1 = ctx.start_inference("foo").unwrap();
        let result = ctx.start_inference("foo");
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Recursive"));
    }
    
    #[test]
    fn test_depth_limit() {
        let mut ctx = InferenceContext::new();
        ctx.max_depth = 3;
        
        let _g1 = ctx.start_inference("f1").unwrap();
        let _g2 = ctx.start_inference("f2").unwrap();
        let _g3 = ctx.start_inference("f3").unwrap();
        
        // Should fail on 4th level
        let result = ctx.start_inference("f4");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("depth limit"));
    }
    
    #[test]
    fn test_guard_cleanup() {
        let mut ctx = InferenceContext::new();
        
        assert!(ctx.can_infer("foo"));
        {
            let _guard = ctx.start_inference("foo").unwrap();
            assert!(!ctx.can_infer("foo"));
        } // Guard dropped here
        assert!(ctx.can_infer("foo")); // Should be cleared
    }
}