# Sable Bootstrap Compiler (Inkwell Backend)

This is an initial compiler implementation aligned to the Sable v2.0 direction:
- deterministic frontend ordering primitives
- explicit effect declarations in signatures
- semantic checking for core language constructs
- typed MIR between sema and backend
- LLVM IR emission through Inkwell

## Current Scope

Implemented baseline features:
- source loading with stable file IDs and span mapping
- lexer with comments, literals, operators, attributes, and effects syntax
- parser for:
  - imports
  - structs
  - functions (including extern signatures)
  - attributes (`@name(...)`)
  - effects clauses (`effects(...)`, `raise(...)`)
  - statements: `let`, `return`, `raise`, `if`, `while`, `for`, `break`, `continue`
  - expressions: arithmetic, comparisons, logical ops, assignment, postfix increment, calls, member and index syntax
  - range operator `..`
- semantic checks for:
  - symbol resolution in local scopes
  - baseline type checking (integers, floats, bool, strings, arrays, refs, named types)
  - call arity and argument type checks
  - effect propagation through calls
  - undeclared effect diagnostics
  - deterministic-function restrictions (`@deterministic` blocks `io` and `unsafe` use)
- typed MIR pipeline:
  - CFG-based MIR with explicit blocks and terminators
  - typed MIR instructions for copies, unary/binary ops, calls, and control-flow lowering
  - deterministic pass pipeline (`constant_fold` -> dead-branch/dead-block elimination)
- LLVM IR codegen (Inkwell) for baseline numeric/control-flow subset

## Not Yet Implemented

Advanced Sable features are intentionally partial in this first slice, including:
- borrow/region checking over CFG
- structured concurrency semantics (`spawn`, `try/catch`, lambdas)
- FFI effect inference and sandbox verification
- hot reload contracts
- full layout/cache/abi attribute semantics
- full MIR-level effect metadata and richer optimization passes

## Build

LLVM 22 is expected locally.

```bash
cargo check
cargo test
```

## CLI

```bash
cargo run -- tokens <file.sable>
cargo run -- ast <file.sable>
cargo run -- check <file.sable>
cargo run -- mir <file.sable>
cargo run -- ir <file.sable>
```

## Baseline Example

A core-feature example is provided at:
- examples/basics.sable

Generate IR:

```bash
cargo run -- ir examples/basics.sable
```
