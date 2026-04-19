# Sable Compiler Progress

Date: 2026-04-19
Repository: sable

## Current Status

The compiler now supports two validated end-to-end flows on Windows 11:

1. Frontend/Sema/MIR + direct execution through a MIR runtime interpreter (`run` command).
2. LLVM IR emission (`ir` command) behind `llvm-backend` with LLVM at `C:\Program Files\LLVM`.

Verified commands:
- `cargo test`
- `cargo run -- run examples/run_showcase.sable`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo check --features llvm-backend`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo run --features llvm-backend -- ir examples/struct_param_member.sable`

Observed `run` output for `examples/run_showcase.sable`:
- `Hello, Sable`
- `true`
- `Hello`
- `2`
- `10`
- `2`
- `program returned: 12`

## Implemented So Far

### 1. Compiler Infrastructure
- Deterministic source loading with stable file IDs.
- Span tracking across lexer/parser/sema/MIR for diagnostics.
- Structured diagnostics with deterministic sort order.

### 2. Frontend (Lexer + Parser)
- Tokenization and parsing for core language forms: structs, functions, attributes, effects, control flow, member/index access, calls, ranges, and typed declarations.
- String and numeric literal support.

### 3. Semantic Analysis (Sema)
- Symbol/type/effect checks for baseline language.
- Loop-context diagnostics:
  - `SEM016`: `break` outside loop.
  - `SEM017`: `continue` outside loop.
- Struct field-order/index metadata for deterministic downstream lowering.
- Builtin member-call typing and effects for:
  - `io.out`
  - `str.concat`, `str.len`, `str.contains`, `str.starts_with`, `str.ends_with`, `str.find`, `str.slice`
  - `vec.new`, `vec.with_capacity`, `vec.push`, `vec.get`, `vec.len`
  - `map.new`, `map.with_capacity`, `map.put`, `map.get`, `map.contains`, `map.len`
  - `ordered_map.new`, `ordered_map.put`, `ordered_map.get`, `ordered_map.contains`, `ordered_map.len`
- String `+` typing support.
- Vector indexing typing support (`vec<T>[index]`).
- Generic type syntax support in declarations: `vec<T>`, `map<K, V>`, `ordered_map<K, V>`.
- Deterministic-context guardrail:
  - direct `map<K, V>` usage is rejected in `@deterministic` functions in favor of `ordered_map<K, V>`.

### 4. MIR Lowering and Optimization
- CFG-based typed MIR lowering for expressions/control flow.
- `for` lowering over fixed-size arrays via index loops (`IndexLoad`).
- Struct metadata (`MirStruct`) propagated into MIR program.
- Builtin call resolution expanded for `io`, `str`, `vec`, `map`, and `ordered_map` runtime intrinsics.
- String constant folding for `+`, `==`, `!=`.
- Default local init for `str` now emits empty string constant.

### 5. Runtime Execution Path
- New MIR runtime interpreter module.
- New CLI command: `run <file.sable>`.
- Runtime builtins implemented:
  - `io.out`
  - `str.concat`, `str.len`, `str.contains`, `str.starts_with`, `str.ends_with`, `str.find`, `str.slice`
  - `vec.new`, `vec.with_capacity`, `vec.push`, `vec.get`, `vec.len`
  - `map.new`, `map.with_capacity`, `map.put`, `map.get`, `map.contains`, `map.len`
  - `ordered_map.new`, `ordered_map.put`, `ordered_map.get`, `ordered_map.contains`, `ordered_map.len`
- Runtime unit test added and passing.

### 6. LLVM Codegen (Windows)
- Backend remains optional behind `llvm-backend`.
- Windows path without `llvm-config` is supported via explicit configuration and linking.
- `IndexLoad` and `MemberLoad` lowering implemented.
- MIR struct metadata now drives named struct type lowering in LLVM backend.
- Verified IR generation on Windows with LLVM prefix configured.

### 7. CLI and Examples
- Commands now supported: `tokens`, `ast`, `check`, `mir`, `run`, `ir`.
- Added end-to-end execution example: `examples/run_showcase.sable`.
- Existing backend validation examples retained:
  - `examples/array_for.sable`
  - `examples/struct_param_member.sable`

### 8. Tests
- All current unit tests pass (6/6), including runtime strings/vectors test.

## Remaining Gaps (Major)

- Generic collection inference is still partial: `vec.new()` / `map.new()` / `ordered_map.new()` may still require annotated target types in some flows.
- Collection key support is intentionally narrow for now (`bool`, integral, `str`) until broader hash/ordering semantics are finalized.
- LLVM backend does not yet lower full runtime-backed string/vector semantics.
- LLVM backend does not yet lower runtime-backed map/ordered_map operations.
- Aggregate store/reference-heavy codegen paths still partial.
- Borrow checker, region checker, and full determinism checker are not yet implemented.
- `try/catch` is still not lowered end-to-end.

## Logical Next Steps

1. Add deeper generic inference so collection constructor calls infer `T`, `K`, and `V` without mandatory annotations.
2. Expand collection APIs (`remove`, `clear`, iteration primitives) with determinism-aware restrictions.
3. Implement integration tests for CLI `run` command using map/ordered_map/string-heavy programs.
4. Continue LLVM parity by lowering runtime-compatible string/vector/map operations to IR (or explicit runtime calls).
5. Complete aggregate stores and ref-based aggregate access in backend.

## Near-Term Milestone

Milestone M2: "Runnable baseline language"
- Goal: core programs with control flow, structs, strings, and vectors compile through sema/MIR and execute via `run` with deterministic behavior.
- Exit criteria:
  - stable `run` outputs for representative examples,
  - integration tests for `run`,
  - documented limits for features not yet codegen-backed in LLVM path.
