# The Sable Language Specification

**Version:** 2.0 (Revised Alpha++)
**Backend:** LLVM
**Philosophy:** *Provable Safety. Explicit Effects. Deterministic Performance.*

---

# 1. Core Principles

## 1.1 Safety by Construction

Sable guarantees:

* memory safety
* type safety
* data race freedom (in safe code)

via a **borrow-checking system** similar to Rust.

Unsafe operations are explicitly marked and optionally verified at runtime (§13).

---

## 1.2 Explicit Effects

All observable side effects are **declared and enforced** at compile time.

There are no hidden:

* allocations
* I/O
* global mutations

---

## 1.3 Deterministic Semantics (Optional Mode)

Sable can enforce **deterministic execution** at the function or module level.

---

## 1.4 Zero-Cost Abstractions

All abstractions:

* compile to predictable LLVM IR
* introduce no hidden allocations or control flow
* preserve layout guarantees

---

## 1.5 Data-Oriented by Default

Memory layout is:

* explicit
* programmable
* optimizable by the compiler

---

# 2. Effects System 

## 2.1 Overview

Functions declare their effects:

```sable
fn read_file(path: str) -> String
    effects(io, alloc)
```

```sable
fn compute(x: i32) -> i32
    effects(none)
```

---

## 2.2 Built-in Effects

| Effect   | Meaning                    |
| -------- | -------------------------- |
| `none`   | pure, no side effects      |
| `alloc`  | heap allocation            |
| `io`     | external I/O               |
| `mut`    | mutation of external state |
| `unsafe` | raw memory / unchecked ops |

---

## 2.3 Rules

* Effects are **transitive**
* Calling a function requires the caller to include its effects
* Violations → compile-time error

---

## 2.4 Function Types

```sable
fn() effects(none)
fn() effects(alloc)
```

Effect sets are part of the type system.

---

## 2.5 Guarantees

> A function declared `effects(none)` is:

* deterministic (unless overridden)
* allocation-free
* side-effect-free

---

# 3. Deterministic Execution

## 3.1 Declaration

```sable
@deterministic
fn simulate(...) { ... }
```

---

## 3.2 Guarantees

* no wall-clock access
* no nondeterministic scheduling
* stable iteration order
* controlled floating-point semantics

---

## 3.3 Restrictions

Forbidden inside deterministic contexts:

* `io`
* `unsafe` (unless explicitly allowed)
* data races
* platform-dependent behavior

---

## 3.4 Enforcement

Compiler rejects:

* effect violations
* nondeterministic constructs

---

# 4. Ownership & Borrowing

## 4.1 Model

Sable uses:

* single ownership
* immutable/mutable borrows
* lifetime inference

---

## 4.2 References

```sable
ref T       // mutable reference
& T         // immutable reference
```

Rules:

* one mutable OR many immutable
* no dangling references

---

## 4.3 Raw Pointers

```sable
ptr<T>
```

* not checked by borrow system
* requires `unsafe`

---

# 5. Regions (First-Class Memory)

## 5.1 Overview

Regions define **allocation domains**:

```sable
region frame

let x = alloc(frame, Player)
```

---

## 5.2 Guarantees

* all allocations tied to region
* region freed in O(1)
* values cannot escape region

---

## 5.3 Borrow Integration

References carry region info:

```sable
ref<frame> Player
```

Compiler enforces:

* no cross-region leaks
* correct lifetimes

---

# 6. Data Layout System

## 6.1 Layout Modes

```sable
@layout(AoS)
@layout(SoA)
```

---

## 6.2 Semantics

* SoA transforms struct into parallel arrays
* access rewritten at compile time
* no runtime overhead

---

## 6.3 Layout Reflection

```sable
@const let l = layout_of(Entity)

l.size
l.align
l.field("pos").offset
```

---

## 6.4 Transformations

```sable
@transform(layout = SoA, align = 64)
struct Entity { ... }
```

---

# 7. Bit-Level Structs

## 7.1 Definition

```sable
@bits
struct Header {
    version: u4
    flags: u2
    len: u26
}
```

---

## 7.2 Guarantees

* exact bit layout
* no padding
* safe field access

---

# 8. Effects + Allocation Model

Allocation is now expressed via effects:

```sable
fn make_vec() -> vec<i32>
    effects(alloc)
```

No special syntax (`!`) required.

---

## 8.1 Guarantee

> Absence of `alloc` effect = zero heap allocation

---

# 9. Unsafe & Verification (Seatbelt Mode)

## 9.1 Unsafe Blocks

```sable
unsafe {
    // raw pointer ops
}
```

---

## 9.2 Verified Unsafe

```sable
unsafe @verify {
    ...
}
```

---

## 9.3 Runtime Checks

In verification mode:

* bounds checking
* lifetime validation
* race detection (optional)

---

## 9.4 Scope

Verification applies:

* only to unsafe regions
* not whole program

---

# 10. Effect-Aware Concurrency (Revised)

## 10.1 Structured Task Model

Sable uses **structured concurrency**.

* tasks are bound to lexical scope
* all spawned tasks must complete before scope exit

---

## 10.2 Task Spawning

```sable
spawn(fn_ptr)
```

### Constraints

A task may be spawned only if:

* captured values are:

  * immutable, or
  * moved, or
  * contained within a transferred region

* no shared mutable state is accessible without synchronization

---

## 10.3 Effect Constraints

* spawning a function requires the caller to include its effect set
* `effects(none)` tasks:

  * require no synchronization primitives
  * are data-race free by construction

---

## 10.4 Pure Task Parallelism

Tasks with `effects(none)`:

* may execute in parallel without locks or atomics
* do not access shared mutable state
* cannot introduce synchronization deadlocks

---

## 10.5 Region Transfer

```sable
spawn(task, move region)
```

Semantics:

* transfers exclusive ownership of the region
* invalidates all references in the sender

Constraints:

* no outstanding borrows allowed
* region must be uniquely owned

---

## 10.6 Deterministic Concurrency

Inside a `@deterministic` context:

* parallel execution is allowed only if results are deterministic

### Guarantees

* identical observable behavior across runs
* stable merge/join order
* no dependence on execution timing

---

## 10.7 Restrictions in Deterministic Contexts

Forbidden:

* mutexes and locks
* unsynchronized shared mutation
* nondeterministic primitives

---

## 10.8 Execution Model

* scheduling is implementation-defined
* observable results are constrained by determinism rules


---

# 11. SIMD & Cache Control

## 11.1 SIMD

```sable
@simd(width = 8)
fn process(...) { ... }
```

---

## 11.2 Cache Alignment

```sable
@cache(line = 64)
struct Particle { ... }
```

---

## 11.3 Tiling

```sable
for tile in data.tile(64) { ... }
```

---

# 12. ABI Contracts

## 12.1 Declaration

```sable
@abi(C)
@verify_layout(size = 16, align = 8)
struct Packet { ... }
```

---

## 12.2 Guarantees

* exact binary layout
* compile-time validation
* cross-platform consistency (or error)

---

# 13. Optimization Contracts
Override CLI flags for marked functions

## 13.1 Declaration

```sable
@opt(level = 3)
fn critical() { ... }
```

---

## 13.2 Deterministic Builds (Optional)

```sable
@opt(level = 3, deterministic = true)
```

Guarantees:

* stable IR generation
* reproducible builds (within toolchain version)

---

# 14. Compile-Time Execution

## 14.1 `@comp`

```sable
@comp let table = generate_table(1024)
```

---

## 14.2 Rules

* no `io`
* no `alloc` unless explicitly allowed
* must be deterministic

---

These fit well with the new direction—*but they need tight constraints* to stay credible and implementable. Below are **drop-in spec entries** written in the same style as your spec, with realistic guarantees and restrictions.

---

# 15. Hot Reloading (State-Preserving)

## 15.1 Overview

Sable supports **state-preserving hot reloading** for eligible functions.

Hot reloading allows replacing a function’s implementation at runtime **without restarting the process** and without corrupting program state.

---

## 15.2 Eligibility

A function is hot-reloadable if it satisfies one of the following:

### Pure Functions

```sable
fn compute(x: i32) -> i32
    effects(none)
```

### Region-Bounded Mutation

```sable
fn update(ref<frame> state: State)
    effects(mut)
```

Constraints:

* mutation must be restricted to explicitly passed regions
* no global state access
* no `unsafe`
* no `io`
* no `alloc` (unless allocation is confined to the same region and proven non-escaping)

---

## 15.3 Declaration

```sable
@hot
fn simulate(...) { ... }
```

---

## 15.4 Semantics

At reload time:

* the function body is replaced
* existing stack frames remain valid
* future calls use the new implementation

For active executions:

* behavior is defined at **function boundary transitions only**
* no mid-instruction replacement

---

## 15.5 Safety Guarantees

The compiler enforces:

* identical function signature
* identical ABI
* compatible layout of all referenced types
* no captured or hidden state

---

## 15.6 Restrictions

Hot reload is rejected if:

* function uses `unsafe`
* function performs unrestricted mutation
* layout of dependent types changes
* control flow depends on external nondeterministic sources

---

## 15.7 Runtime Behavior

Reloading produces:

* atomic function pointer swap
* optional rollback on failure
* versioned function table

---

## 15.8 Guarantee

> A valid hot reload will not:

* corrupt memory
* invalidate references
* introduce undefined behavior

---

# 16. Semantic Binary Patching

## 16.1 Overview

Sable supports **semantic binary patch generation** for efficient deployment.

Instead of distributing full binaries, the compiler can emit **verified patch diffs**.

---

## 16.2 Requirements

Patch generation requires:

```sable
@abi(C)
@verify_layout(...)
```

on all externally visible types.

---

## 16.3 Patch Generation

```
sable build --patch-from=prev_build
```

Produces:

* minimal binary diff
* metadata describing structural compatibility

---

## 16.4 Verification

The compiler ensures:

* identical struct layouts
* unchanged field offsets
* compatible alignment
* stable symbol signatures

---

## 16.5 Rejection Conditions

Patch generation fails if:

* struct size changes
* field order changes
* ABI contracts are violated
* function signatures differ incompatibly

---

## 16.6 Application

At runtime or deployment:

* patch is applied atomically
* integrity is verified before application
* rollback is supported

---

## 16.7 Use Cases

* embedded systems
* IoT devices
* space systems with bandwidth constraints

---

## 16.8 Guarantee

> A valid patch:

* preserves memory layout correctness
* does not introduce ABI inconsistencies
* is safe to apply to a running or static system

---

# 17. Deterministic Recording & Replay

## 17.1 Overview

Sable provides **exact execution replay** for deterministic code.

A deterministic module can be recorded using only its **initial inputs**, enabling perfect reproduction of execution.

---

## 17.2 Requirements

The target must be:

```sable
@deterministic
module simulation { ... }
```

---

## 17.3 Recorder API

```sable
import std.recorder

let recording = recorder.capture(simulation.run, input)
```

---

## 17.4 Stored Data

Recorder stores:

* initial input values
* version metadata
* optional execution checksum

No runtime trace is required.

---

## 17.5 Replay

```sable
recorder.replay(recording)
```

---

## 17.6 Guarantees

Replay execution is:

* instruction-order identical
* data-flow identical
* bitwise reproducible

---

## 17.7 Enforcement

Compiler ensures:

* no wall-clock access
* no randomness
* no nondeterministic scheduling
* stable iteration order
* deterministic floating-point behavior

---

## 17.8 Failure Modes

Replay is rejected if:

* binary version mismatch (unless explicitly allowed)
* deterministic guarantees are violated
* environment constraints differ (e.g., architecture mismatch if not normalized)

---

## 17.9 Use Cases

* debugging complex simulations
* reproducing rare bugs
* testing codecs and numerical systems

---

## 17.10 Guarantee

> A valid recording will reproduce the exact same execution behavior across runs.

---

# 18. Foreign Function Interface (FFI) & Effect Inference

## 18.1 Overview

Sable supports foreign function calls with **compile-time effect inference**.

Foreign code is analyzed to determine its **observable side effects**, ensuring integration with Sable’s effect system.

---

## 18.2 Import Declaration

```sable
extern "C" fn c_func(x: i32) -> i32
```

---

## 18.3 Static Effect Inference

The compiler performs static analysis on the imported symbol (if available):

### Detected Effects

* memory allocation
* I/O (syscalls)
* global memory mutation
* pointer writes
* unsafe operations

---

## 18.4 Pessimistic Fallback

If full analysis is not possible, the compiler assigns:

```sable
effects(alloc, io, mut, unsafe)
```

This is the **maximal effect set**.

---

## 18.5 Explicit Effect Annotations

The user may declare effects manually:

```sable
extern "C" fn fast_sin(x: f32) -> f32
    effects(none)
```

---

## 18.6 Verification Requirement

User-declared effects must be validated via one of:

### 1. Static Proof (if possible)

* compiler verifies annotation matches analysis

### 2. Runtime Verification

```sable
extern "C" fn fast_sin(x: f32) -> f32
    effects(none)
    @verify
```

Seatbelt-lite checks:

* no allocation
* no I/O
* no mutation

---

## 18.7 Sandbox Execution

Sable provides a restricted execution wrapper:

```sable
sandbox {
    fast_sin(x)
}
```

### Guarantees (in verification mode):

* no syscalls escape
* no global memory writes
* no allocation outside allowed regions
* pointer access is tracked

---

## 18.8 Unsafe Override

If the user bypasses verification:

```sable
extern "C" fn fast_sin(x: f32) -> f32
    effects(none)
    unsafe
```

Then:

* compiler trusts the annotation
* responsibility is on the programmer

---

## 18.9 Effect Propagation

Inferred or declared effects propagate normally:

```sable
fn wrapper(x: f32) -> f32
    effects(none)
{
    return fast_sin(x) // must match effects
}
```

---

## 18.10 Guarantees

> If effect inference or verification succeeds:

* foreign code respects declared effects
* safety guarantees are preserved

> If `unsafe` override is used:

* behavior is not guaranteed
* violations may occur


---

# 19. Effect-Aware Error Handling

## 19.1 Overview

Sable models error propagation as an **explicit effect**.

A function that may fail must declare the set of error types it can raise. Errors are part of the function’s type and are enforced at compile time.

---

## 19.2 The `raise` Effect

### Declaration

```sable
fn save_config(data: Config) -> void
    effects(io, raise(FileError))
```

### Semantics

* `raise(E)` indicates the function may terminate by raising an error of type `E`
* multiple error types are allowed:

```sable
effects(raise(FileError, ParseError))
```

---

## 19.3 Type System Integration

The `raise` effect is part of the function type:

```sable
fn() -> void effects(raise(FileError))
```

Rules:

* callers must handle or propagate all raised errors
* missing handling → compile-time error
* effect sets must match across assignments and calls

---

## 19.4 Raising Errors

Errors are raised explicitly:

```sable
raise FileError.NotFound
```

Semantics:

* immediately exits the current function
* transfers control to the nearest matching handler

---

## 19.5 Handling Errors

### Try/Catch

```sable
try {
    save_config(cfg)
} catch (e: FileError) {
    handle(e)
}
```

### Rules

* catching an error **removes it from the effect set**
* unhandled errors must be propagated

---

## 19.6 Propagation

Errors propagate automatically if not handled:

```sable
fn wrapper() -> void
    effects(raise(FileError))
{
    save_config(cfg)
}
```

---

## 19.7 Effect Constraints on Handling

A handler cannot introduce new effects beyond its enclosing scope:

```sable
fn process(data: str) effects(none) {
    try {
        parse(data)
    } catch (e: ParseError) {
        // allowed (no new effects)
    }
}
```

Invalid:

```sable
fn process(data: str) effects(none) {
    try {
        parse(data)
    } catch (e: ParseError) {
        save_to_disk() // introduces io → compile error
    }
}
```

---

## 19.8 Recoverable vs Fatal Errors

### 19.8.1 Recoverable Errors (`raise`)

* must be declared in effects
* must be handled or propagated
* part of the type system

---

### 19.8.2 Fatal Errors (`abort`)

Used for unrecoverable failures:

```sable
abort("out of memory")
```

Semantics:

* immediately terminates execution of the current task
* does not require declaration in effects

---

### 19.8.3 Behavior

* outside managed contexts → program termination
* inside `sandbox` or `@hot`:

  * triggers controlled rollback or shutdown
  * does not corrupt memory or state

---

## 19.9 Deterministic Error Handling

Inside a `@deterministic` context:

* error behavior must depend only on:

  * function inputs
  * deterministic state

### Guarantees

* identical inputs → identical success or failure
* identical error type and value

---

### Restrictions

Forbidden:

* transient failures (timeouts, system state)
* nondeterministic error sources

Unless explicitly injected as deterministic inputs.

---

## 19.10 Error Composition

Functions may raise multiple error types:

```sable
effects(raise(IOError, ParseError))
```

---

### Matching

```sable
catch (e: IOError) { ... }
catch (e: ParseError) { ... }
```

---

### Exhaustiveness

The compiler enforces:

* all raised error types are handled or propagated
* no silent error loss

---

## 19.11 Interaction with Effects

Error handling integrates with the effect system:

* `raise` composes with other effects (`io`, `alloc`, etc.)
* effect inference includes error propagation

---

## 19.12 Tooling Guarantees

The compiler can query:

* all functions that may fail
* all unhandled error paths
* all I/O operations without error propagation

---

## 19.13 Hot Reload Interaction

For `@hot` functions:

* if a new implementation raises an error not present in the previous version:

  * reload is rejected, or
  * runtime performs rollback

---

## 19.14 Guarantees

A well-typed Sable program guarantees:

* no unhandled recoverable errors
* no hidden control flow via exceptions
* fully traceable failure paths
* deterministic error behavior (when enabled)

---








