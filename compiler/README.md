# Sable Bootstrap Compiler

This is an initial compiler implementation aligned to the Sable v2.0 direction:
- deterministic frontend ordering primitives
- explicit effect declarations in signatures
- semantic checking for core language constructs
- typed MIR between sema and backend
- MIR interpreter execution path via `run`
- optional LLVM IR emission through Inkwell (`llvm-backend` feature)

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
- builtin type/effect checking for:
  - `io.out`
  - `str.concat`, `str.len`, `str.contains`, `str.starts_with`, `str.ends_with`, `str.find`, `str.slice`
  - `vec.new`, `vec.with_capacity`, `vec.push`, `vec.get`, `vec.len`
  - `map.new`, `map.with_capacity`, `map.put`, `map.get`, `map.contains`, `map.len`
  - `ordered_map.new`, `ordered_map.put`, `ordered_map.get`, `ordered_map.contains`, `ordered_map.len`
- typed MIR pipeline:
  - CFG-based MIR with explicit blocks and terminators
  - typed MIR instructions for copies, unary/binary ops, calls, and control-flow lowering
  - deterministic pass pipeline (`constant_fold` -> dead-branch/dead-block elimination)
- runtime execution pipeline:
  - MIR interpreter for a bootstrap execution path (`cargo run -- run <file.sable>`)
  - current runtime builtins for strings, `vec<T>`, `map<K,V>`, and `ordered_map<K,V>`
- LLVM IR codegen (Inkwell) for baseline numeric/control-flow subset when `llvm-backend` is enabled
- LLVM IR lowering for array index loads and struct member loads in the current subset

## Not Yet Implemented

Advanced Sable features are intentionally partial in this first slice, including:
- borrow/region checking over CFG
- structured concurrency semantics (`spawn`, `try/catch`, lambdas)
- FFI effect inference and sandbox verification
- hot reload contracts
- full layout/cache/abi attribute semantics
- full MIR-level effect metadata and richer optimization passes

Notes on collection semantics:
- `map<K, V>` uses hash-based lookup for high-performance average-case access.
- `ordered_map<K, V>` uses ordered keys and stable iteration semantics; deterministic mode can target this without redesign.
- Key types for map builtins are currently limited to `bool`, integer types, and `str`.

## Build

Default build (frontend + sema + MIR) does not require a local LLVM install.

```bash
cargo check
cargo test
cargo run -- run examples/run_showcase.sable
```

Enable LLVM IR backend explicitly when needed:

```bash
cargo check --features llvm-backend
cargo run --features llvm-backend -- ir examples/basics.sable
```

## Windows notes

Windows now works out of the box for non-IR commands (`tokens`, `ast`, `check`, `mir`) without LLVM.

To use the `ir` command on Windows, install a compatible LLVM 22 toolchain and point `llvm-sys` to it via `LLVM_SYS_221_PREFIX` if needed.

For this repository's current Windows setup, this works:

```powershell
$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"
cargo run --features llvm-backend -- ir examples/basics.sable
cargo run --features llvm-backend -- ir examples/array_for.sable
```

- Recommended: install LLVM (for example via Chocolatey):

```powershell
choco install llvm
```

- Or install `llvmenv` and use it to provide a local LLVM copy for building:

```powershell
cargo install llvmenv
llvmenv install 22
llvmenv activate 22
```

## CLI

```bash
cargo run -- tokens <file.sable>
cargo run -- ast <file.sable>
cargo run -- check <file.sable>
cargo run -- mir <file.sable>
cargo run -- run <file.sable>
cargo run -- ir <file.sable>
```

The `ir` command requires `--features llvm-backend`.

## Baseline Example

A core-feature example is provided at:
- examples/basics.sable
- examples/array_for.sable
- examples/struct_param_member.sable
- examples/run_showcase.sable

Generate IR:

```bash
cargo run -- ir examples/basics.sable
```
