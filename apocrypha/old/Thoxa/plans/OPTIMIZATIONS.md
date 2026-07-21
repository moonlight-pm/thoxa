# Future Optimizations

This document tracks potential optimizations and improvements that we may implement once the core language is working.

## Primitive Type Optimization

### Problem
Our uniform 16-byte value representation (type pointer + 64-bit data) creates overhead for primitive types that could be stored more efficiently, especially in arrays and structs.

### Potential Solutions

#### 1. Type Classes/Kinds
Distinguish between primitive and heap types at the Type level:
- Primitive types carry QBE type information ('w', 'l', 's', 'd')
- Enable unboxed storage in aggregates
- Generate efficient QBE code for primitives

#### 2. Unboxed Arrays
Store primitive arrays efficiently:
```
Array(Int32, 1000) stores 4KB of data, not 16KB
```

#### 3. Primitive Type Singletons
Pre-allocate global primitive type objects:
- Fast pointer comparison still works
- Can check if type is primitive via pointer range
- Enables specialized code generation

#### 4. Hybrid Value Representation
- Standalone values: 16-byte boxed representation
- Array/struct fields: Unboxed primitive storage
- Compiler handles boxing/unboxing transparently

### Benefits
- Memory efficiency for large arrays
- Better cache performance
- More efficient QBE code generation
- No user-visible API changes

### Implementation Notes
- Start with uniform representation for simplicity
- Profile real programs to identify bottlenecks
- Implement optimizations incrementally
- Maintain semantic equivalence

## User-Defined Generic Types

### Design Decisions (Deferred)

When we implement user-defined generics, the plan is:

1. **Type Erasure** (initial implementation)
   - Single compiled function/type for all instantiations
   - Types checked at compile time, erased at runtime
   - Simpler to implement than monomorphization
   - May switch to monomorphization later for performance

2. **Interface Constraints**
   ```thoxa
   fn sort<T: Comparable>(items: Array<T>) { ... }
   
   type Container<T: Serializable> {
       items: Array<T>
   }
   ```

3. **Type Inference**
   - Follow same rules as rest of language
   - Infer generic parameters where possible
   - Explicit annotation always allowed

4. **No Variance**
   - Keep it simple
   - `Array<Dog>` is not a subtype of `Array<Animal>`
   - Avoid complexity of covariance/contravariance

### Implementation Notes
- Start with built-in generics only (Ptr<T>, Array<T>)
- Design APIs to support future user generics
- Consider monomorphization as optimization later

## Other Future Considerations

### Memory Management
- Garbage collection strategies
- Reference counting optimization
- Arena allocation for types

### Compilation Performance
- Incremental compilation
- Parallel compilation
- Type caching across compilation units

### Language Features
- Module system design
- Pattern matching

### Runtime Performance  
- Inline caching for dynamic dispatch
- Specialized instructions for common operations
- JIT compilation for hot paths (would require different backend)