# light_machine VM Specification

This document defines the `light_machine` virtual machine, its memory layout,
stack model, calling convention, and opcode semantics. It is the authoritative
reference for code generation, assemblers, and runtime behavior.

## Word size and types

- `ProgramWord` is `u16` (unsigned 16-bit).
- `StackWord` is `u32` (unsigned 32-bit).
- Program image words (header, tables, opcodes, immediates, static data) are
  `ProgramWord`.
- Runtime memory words (globals and stack) are `StackWord`.
- Machine indices, function indices, and program addresses are encoded as
  `ProgramWord` and interpreted as `usize` when indexing.

## Program image layout

Programs are stored in a single contiguous `ProgramWord` array (`static_data`).
The current supported program version is `2`.

The program header:

```
[0] VERSION
[1] MACHINE_COUNT (instance count)
[2] GLOBALS_SIZE (total globals required for all instances, in `StackWord` cells)
[3] SHARED_FUNCTION_COUNT
[4] TYPE_COUNT
[5] INSTANCE_TABLE_OFFSET
[6] TYPE_TABLE_OFFSET
[7] SHARED_FUNCTION_TABLE_OFFSET
```

The instance table entries point to a machine type and globals base offset.
The type table entries point to a type-local function table.
The shared function table entries point to program-scoped shared function entry
points.

## Runtime memory contract

`Program::new` takes:

- `static_data: &[ProgramWord]`
- `memory: &mut [StackWord]`

`memory` is split into globals storage and stack storage:

- Globals start at cell `0` and occupy `GLOBALS_SIZE` `StackWord` cells.
- Stack starts immediately after globals.
- If `memory` does not provide enough cells for required globals plus runtime
  stack capacity, construction fails with `MemoryBufferTooSmall`.

## Instance + type tables

Instance table layout (at `INSTANCE_TABLE_OFFSET`):

```
For each instance `i`:
  [0] TYPE_ID          ; index into the type table
  [1] GLOBALS_BASE     ; offset into the globals buffer
```

Type table layout (at `TYPE_TABLE_OFFSET`):

```
For each type `t`:
  [0] FUNCTION_COUNT
  [1] FUNCTION_TABLE_OFFSET
```

Function index convention used by host helpers:

- `0 = init`
- `1 = start_frame`
- `2 = get_color`
- remaining are user-defined.

Host render-loop call order is:

1. `init` once when the program is initialized.
2. `start_frame(tick)` once per machine per frame/timestep.
3. `get_color(index)` once per machine for each LED in the frame.

Function tables (pointed to by `FUNCTION_TABLE_OFFSET`) are sequences of entry
points into `static_data`:

```
[entry_point_0, entry_point_1, ... entry_point_n]
```

Shared function table layout (at `SHARED_FUNCTION_TABLE_OFFSET`):

```
[shared_entry_0, shared_entry_1, ... shared_entry_n]
```

Builder note: `ProgramBuilder` allocates function/shared-function tables from
declared counts, but does not perform a final completeness validation that every
slot was explicitly defined.

## I2C shared function IDs

When a program is intended to run with firmware I2C integration, shared
function indices are reserved:

- `0`: init program
- `1`: get I2C routes
- `2`: add I2C route
- `3`: remove I2C route

These indices are a contract with firmware and UI.

## Program graph emission

Compilers may build a program graph of static data, functions, machine types,
and machine instances before emitting a program. The graph is used to dedupe
identical definitions (statics, functions, and types) while keeping instance
ordering stable. Emission still produces the same runtime tables (instances,
types, shared functions); the graph only affects sharing and final layout.

Implementation notes (current):

- The graph is built in `flight-deck` and emitted into `ProgramBuilder`.
- Function bodies are stored as word references (literals, static refs, and
  label offsets). Label offsets are resolved to absolute addresses at emit time.
- Per-machine and shared static data are emitted into shared static storage so
  all static addresses are global within `static_data`.

## Stack model

- The VM is stack-based and uses `StackWord` values.
- Most operations pop operands and push results.
- Stack underflow/overflow are runtime errors.

### Frame pointer

The VM maintains `frame_pointer` (a `StackWord` interpreted as checked
`usize`). `SLOAD` and `SSTORE` are relative to `frame_pointer`.

### Machine locals pointer

The VM maintains a machine locals base pointer (`mlp`).

- For machine functions, `mlp` is the instance `GLOBALS_BASE`.
- For shared functions called by `CALL_SHARED`, `mlp` is inherited from the
  calling machine.
- For host-initiated `Program::call_shared`, current implementation executes
  with machine index `0` (`mlp` from instance 0).

`LLOAD` and `LSTORE` use `mlp + offset`.

## Calling convention

`CALL` and `CALL_SHARED` both expect:

```
... arg0 arg1 ... argN-1 arg_count func_index
```

Semantics:

1. Pop `func_index`.
2. Pop `arg_count`.
3. Compute `arg_start = stack_len - arg_count`.
4. Insert `return_pc` at `arg_start`.
5. Insert `saved_frame_pointer` at `arg_start + 1`.
6. Set `frame_pointer = arg_start + 2` (points to `arg0`).
7. Jump to target entry point (type function table for `CALL`, shared function
   table for `CALL_SHARED`).
8. On return from nested `run`, restore caller `frame_pointer` and resume at
   `return_pc`.

`RET <count>` uses current `frame_pointer` to locate saved `return_pc` and saved
frame pointer. It copies `<count>` values from top-of-stack, removes frame
header/body, restores `frame_pointer`, pushes copied values, and sets
`pc = return_pc`.

## Instruction encoding

- Each instruction is one `ProgramWord` opcode.
- Some instructions consume one immediate `ProgramWord`.
- Assembler convenience expansion:
  - `CALL <x>` => `PUSH <x>` + `CALL`
  - `CALL_SHARED <x>` => `PUSH <x>` + `CALL_SHARED`
  - `JUMP <x>` => `PUSH <x>` + `JUMP`
  - `BR* <x>` => `PUSH <x>` + `BR*`

For `CALL`/`CALL_SHARED`, caller must still push `arg_count` beneath
`func_index`.

## Opcode table

All of the following are implemented.

### Stack and data ops

- `PUSH <word>`: push immediate.
- `POP`: pop and discard top.
- `DUP`: duplicate top.
- `SWAP`: swap top two values.
- `SLOAD <offset>`: push `stack[frame_pointer + offset]`.
- `SSTORE <offset>`: store top at `stack[frame_pointer + offset]`, then pop top.
- `LLOAD <offset>`: push `globals[mlp + offset]`.
- `LSTORE <offset>`: pop and store to `globals[mlp + offset]`.
- `GLOAD <addr>`: push `globals[addr]`.
- `GSTORE <addr>`: pop and store to `globals[addr]`.
- `LOAD_STATIC`: pop `addr`, push `static_data[addr]`.

Assembler/builders currently constrain `GLOAD`/`GSTORE` operands to declared
shared-global addresses. Runtime executes whatever address is encoded.

### Control flow

- `JUMP`: pop `addr`, set `pc = addr`.
- `CALL`: pop function index and arg count, build frame, jump to type function.
- `CALL_SHARED`: pop shared-function index and arg count, build frame, jump to
  shared function.
- `RET <count>`: restore frame state, return `<count>` values, jump to saved PC.
- `BRLT`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs < rhs`, jump to `addr`.
- `BRLTE`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs <= rhs`, jump to `addr`.
- `BRGT`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs > rhs`, jump to `addr`.
- `BRGTE`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs >= rhs`, jump to `addr`.
- `BREQ`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs == rhs`, jump to `addr`.
- `EXIT`: return from the current `run` invocation without frame unwind.

### Logical ops

- `AND`: pop `lhs`, `rhs`; push `1` if both non-zero else `0`.
- `OR`: pop `lhs`, `rhs`; push `1` if either non-zero else `0`.
- `XOR`: pop `lhs`, `rhs`; push `1` if exactly one is non-zero else `0`.
- `NOT`: pop `value`; push `1` if zero else `0`.

### Bitwise ops

- `BAND`: pop `lhs`, `rhs`; push `lhs & rhs`.
- `BOR`: pop `lhs`, `rhs`; push `lhs | rhs`.
- `BXOR`: pop `lhs`, `rhs`; push `lhs ^ rhs`.
- `BNOT`: pop `value`; push `!value`.

### Arithmetic ops

- `ADD`: pop `lhs`, `rhs`; push `lhs + rhs` (wrapping).
- `SUB`: pop `lhs`, `rhs`; push `lhs - rhs` (wrapping).
- `MUL`: pop `lhs`, `rhs`; push `lhs * rhs` (wrapping).
- `DIV`: pop `lhs`, `rhs`; push `lhs / rhs`.
- `MOD`: pop `lhs`, `rhs`; push `lhs % rhs`.

Implementation detail: division/modulo by zero currently surface as
`MachineError::InvalidOp(<opcode_word>)`.

## Opcode numeric encoding

Current `ProgramWord` opcode mapping:

- `0 POP`
- `1 PUSH`
- `2 BRLT`
- `3 BRLTE`
- `4 BRGT`
- `5 BRGTE`
- `6 BREQ`
- `7 AND`
- `8 OR`
- `9 XOR`
- `10 NOT`
- `11 BAND`
- `12 BOR`
- `13 BXOR`
- `14 BNOT`
- `15 MUL`
- `16 DIV`
- `17 MOD`
- `18 ADD`
- `19 SUB`
- `20 LLOAD`
- `21 LSTORE`
- `22 GLOAD`
- `23 GSTORE`
- `24 LOAD_STATIC`
- `25 JUMP`
- `26 EXIT`
- `27 CALL`
- `28 CALL_SHARED`
- `29 SLOAD`
- `30 SSTORE`
- `31 DUP`
- `32 SWAP`
- `33 RET`

## Runtime error conditions

Runtime can report:

- `InvalidProgramVersion`
- `InvalidOp`
- `OutOfBoudsStaticRead`
- `OutOfBoundsGlobalsAccess`
- `PopOnEmptyStack`
- `StackUnderFlow`
- `StackOverflow`
- `TwoFewArguments`
- `GlobalsBufferTooSmall`
- `MemoryBufferTooSmall`
- `MachineIndexOutOfRange`
- `SharedFunctionIndexOutOfRange`
- `StackValueTooLargeForProgramWord`
- `StackValueTooLargeForUsize`
- `ColorOutOfRange` (used by `get_led_color` host helper).

## Notes

- `EXIT` is a simple return from `run` and does not unwind call frames.
  Use `RET <count>` inside called functions.
- Entry points invoked from host should normally end with `EXIT` unless they are
  only reached through `CALL`/`CALL_SHARED`.
- Assembler mnemonics are case-insensitive.
