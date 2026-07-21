# Arrays Implementation Plan

## Overview
Arrays are dynamically-sized, homogeneous collections with automatic memory management via ARC.

## Phase 1: Core Array Support ✅

### 1.1 Array Literals ✅
```thoxa
let nums = [1, 2, 3];           # Array<Int>
let strings = ["a", "b", "c"];  # Array<String>
let empty: Array<Int> = [];     # Empty with explicit type
```

**Tasks:**
- [x] Generate array allocation with `thoxa_array_new`
- [x] Generate element insertion for each literal element
- [x] Type check all elements are same type
- [x] Infer array element type from first element
- [x] Handle empty arrays (require type annotation)

### 1.2 Array Indexing ✅
```thoxa
let first = nums[0];            # Get element
nums[1] = 42;                   # Set element (if mut)
```

**Tasks:**
- [x] Generate bounds checking with `thoxa_check_bounds`
- [x] Generate `thoxa_array_get` calls for reading
- [x] Generate `thoxa_array_set` calls for assignment
- [x] Proper type handling for element access
- [x] Check mutability for array assignment

### 1.3 Array Properties ✅
```thoxa
let len = nums.count;           # Get array element count
```

**Tasks:**
- [x] Implement `.count` property via `thoxa_array_length`
- [x] Support for both old `Ptr` type and new `[T]` array types

### 1.4 Array Type System ✅
```thoxa
let nums: [Int] = [1, 2, 3];           # Explicitly typed array
let maybes = [Has(1), Not(), Has(3)];  # Array of May<Int>
```

**Tasks:**
- [x] Array type representation in type system (`[T]`)
- [x] Element type tracking through all operations
- [x] Type inference for array literals
- [x] Proper type handling in for-in loops
- [x] Array indexing returns correct element type

## Phase 2: Array Methods

### 2.1 Basic Mutating Methods
```thoxa
mut nums = [1, 2, 3];
nums.push(4);                   # Add to end
let last = nums.pop();          # Remove from end (returns May<T>)
nums.clear();                   # Remove all elements
```

**Tasks:**
- [ ] Implement `push` method - `thoxa_array_push`
- [ ] Implement `pop` method - returns `May<T>`
- [ ] Implement `clear` method - `thoxa_array_clear`
- [ ] Ensure methods check mutability

### 2.2 Additional Methods (Future)
```thoxa
nums.insert(2, value);          # Insert at position
nums.remove(2);                 # Remove at position
let sub = nums.slice(1, 3);    # Get subarray [start, end)
nums.reverse();                 # Reverse in place
nums.sort();                    # Sort in place (if T is comparable)
```

## Phase 3: Iteration

### 3.1 For-in Loops ✅
```thoxa
for num in nums {
    println(num);
}
```

**Tasks:**
- [x] Implement for-in syntax for arrays
- [x] Generate index-based iteration (transforms to while loop)
- [x] Handle reference counting during iteration
- [x] Support both inline arrays and variable identifiers

### 3.2 Enumeration (Future)
```thoxa
for (i, num) in nums.enumerate() {
    println("{i}: {num}");
}
```

## Phase 4: Advanced Features

### 4.1 Multi-dimensional Arrays
```thoxa
let matrix = [[1, 2], [3, 4]]; # Array<Array<Int>>
let val = matrix[0][1];         # Chained indexing
matrix[1][0] = 5;              # Nested assignment
```

**Tasks:**
- [ ] Support nested array types
- [ ] Chain indexing operations
- [ ] Proper reference counting for nested arrays

### 4.2 Slicing
```thoxa
let subset = nums[1:3];         # Elements at indices 1 and 2
let rest = nums[2:];           # From index 2 to end
let copy = nums[:];            # Full copy
```

**Tasks:**
- [ ] Parse range syntax in index position
- [ ] Generate efficient slice copying
- [ ] Handle negative indices (from end)

## Type System Integration

### Type Checking Rules
1. All array elements must be the same type
2. Empty arrays require type annotation
3. Array type is invariant (Array<Int> ≠ Array<Float>)
4. Arrays are heap-allocated with reference counting

### Memory Layout
```
Array<T> structure:
[-8] Reference count
[0]  Type pointer (ARRAY_TYPE)
[8]  Header flags
[16] Length (size_t)
[24] Capacity (size_t)  
[32] Element size (size_t)
[40] Element type (uint16_t)
[48] Data pointer (void*)
```

## Implementation Status
1. **Complete**: Array literals and basic indexing ✅
2. **Complete**: For-in loops ✅  
3. **Complete**: Array `.count` property ✅
4. **Complete**: Array methods (push, pop, clear) ✅
   - `push(element)`: Add element to end
   - `pop() -> May<T>`: Remove and return last element (returns `Not()` if empty)
   - `clear()`: Remove all elements
5. **Future**: Multi-dimensional arrays, slicing, advanced methods

### Known Limitations
- **Mutability Checking**: The compiler checks that only mutable arrays can call mutating methods (`push`, `pop`, `clear`), but this check only works when the array variable has been previously defined. It doesn't catch mutations in the same statement as initialization (e.g., `let arr = [1,2,3]; arr.push(4);` in a single expression context).

## Testing Strategy
1. Basic array creation and indexing
2. Type checking (homogeneous elements)
3. Bounds checking
4. Memory management (reference counting)
5. Nested arrays
6. Large arrays (growth/reallocation)

## Example Test Cases
```thoxa
# Basic operations
fn testArrayBasics() {
    let nums = [1, 2, 3];
    assert(nums.count == 3);
    assert(nums[0] == 1);
    assert(nums[2] == 3);
}

# Mutation
fn testArrayMutation() {
    mut nums = [1, 2, 3];
    nums.push(4);
    assert(nums.count == 4);
    assert(nums[3] == 4);
    
    let last = nums.pop();
    assert(last == Has(4));
    assert(nums.count == 3);
}

# Type checking
fn testArrayTypes() {
    let ints = [1, 2, 3];
    let floats = [1.0, 2.0, 3.0];
    # let mixed = [1, 2.0, 3];  # Should error
}

# Iteration
fn testArrayIteration() {
    let nums = [1, 2, 3];
    let mut sum = 0;
    for n in nums {
        sum += n;
    }
    assert(sum == 6);
}
```

## Notes
- Arrays use 0-based indexing
- Out-of-bounds access causes runtime panic (with bounds checking)
- Arrays automatically grow when needed (via push)
- No array comprehensions (by design decision)
- Mutability is controlled by `mut` keyword on the binding