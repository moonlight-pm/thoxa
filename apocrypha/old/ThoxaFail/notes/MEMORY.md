 Stack Allocation

  - Fastest - just moves stack pointer
  - Automatic cleanup - freed when function returns
  - Limited size - typically 1-8MB per thread
  - LIFO order - last allocated, first freed
  - Used for: local variables, function parameters, return addresses

  Heap Allocation

  - Flexible size - can allocate large amounts
  - Manual management - must explicitly free (or use GC)
  - Slower - requires allocator bookkeeping
  - Fragmentation possible - can waste space over time
  - Used for: dynamic data structures, large objects, data with unknown lifetime

  Static/Global Memory

  - Compile-time allocation - size known at compilation
  - Program lifetime - exists for entire program run
  - No allocation cost - allocated when program loads
  - Used for: constants, global variables, string literals

  Memory-Mapped (mmap)

  - Large allocations - good for huge data (GB+)
  - Page-aligned - works with OS virtual memory
  - Can be file-backed - map files directly to memory
  - Used for: databases, large files, shared memory

  Arena/Bump Allocators

  - Very fast allocation - just bump a pointer
  - Bulk deallocation - free entire arena at once
  - No individual free - can't free single objects
  - Used for: temporary allocations, compiler passes, game frames

  Pool Allocators

  - Fixed-size objects - all allocations same size
  - No fragmentation - perfect memory reuse
  - Fast alloc/free - O(1) operations
  - Used for: particles, network packets, object pools

  Custom Allocators (Zig-style)

  In Zig, you explicitly pass allocators, allowing:
  - Testing - use fixed buffer to test memory usage
  - Performance - arena for temp work, general purpose for long-lived
  - Safety - track allocations, detect leaks
  - Special needs - aligned memory, specific memory regions

  In Cranelift Context

  Cranelift primarily deals with:
  1. Stack slots - for function locals via stack_slot operations
  2. Constants - in read-only data section
  3. Heap pointers - you generate calls to malloc/free or custom allocators
  4. Static data - via data declarations

  The key insight: Cranelift doesn't provide allocators - it generates code that calls whatever allocation strategy you implement in your runtime.
