# Light Machine Assembly Language

## Purpose

This is a small assembly format for the Light Machine VM.
It is intended for hand-authoring programs and easy evolution of the
instruction set. The assembler should be a thin mapping from mnemonics
to opcodes and word data.

## Design goals

- Simple, line-oriented syntax.
- Easy to add or rename opcodes in one table.
- Minimal syntax sugar; keep it close to the VM's u16 program word stream.
- Labels for readability.
- A clear place for locals size, machines, and functions.

## File structure

A source file is a sequence of directives and instructions.

Directives (top-level):

- `.machine <name> locals <N> functions <M>`: starts a new machine (locals are per-machine state).
- `.func <name> [index <I>]`: starts a new function within the current machine.
- `.func_decl <name> [index <I>]`: declares a function without a body.
- `.data <name>`: starts a static data block (u16 program words).
- `.shared_func <name> [index <I>]`: starts a program-scoped shared function.
- `.shared_func_decl <name> [index <I>]`: declares a shared function without a body.
- `.shared_data <name>`: starts a program-scoped static data block.
- `.shared <name> <index>`: declares a named shared global index (program-scoped).
- `.frame <name> <offset>`: declares a named stack slot for SLOAD/SSTORE.
- `.end`: ends the current machine, function, or data block.

Directives (machine-level):

- `.local <name> <index>`: declares a named local index for LLOAD/LSTORE.

Notes:

- `<N>` and `<M>` are u16 values (program words).
- `<name>` is currently informational; the assembler does not emit it.
- `index <I>` is optional; if omitted, functions are assigned in order.
- `.func_decl` reserves an index and allows forward references in a one-pass
  assembler. A later `.func` with the same name must provide the body.
- `.data` blocks can appear anywhere inside a machine and can be referenced by
  labels when `LOAD_STATIC` is implemented.
- `.shared` must be declared before any `.machine`.
- `.machine` accepts `globals` as a deprecated alias for `locals`.
- `LLOAD`/`LSTORE` numeric operands are treated as local offsets; use `.shared` labels with `GLOAD`/`GSTORE` for shared state.
- Labels are allowed in functions and data blocks.
  Inside `.data`, either use `.word <number>` or a bare `<number>` per line.

## Shared functions

Shared functions are program-scoped function bodies callable from any machine.
See `FluxPilot/crates/light_machine/shared_functions_plan.md` for the format,
semantics, and header layout details.

## Comments

Use `;` for line comments.

## Numbers

All numbers in the assembly source are u16 program words unless otherwise specified.
Stack values are u32 at runtime.
Supported formats:

- Decimal: `123`
- Hex: `0x7B`

## Labels

Labels end with `:` and can be referenced by name.
Labels resolve to a word index within the current function or data block.

## Instruction syntax

One instruction per line:

    <MNEMONIC> [operand]

Operands are u16 program words or label references, depending on the opcode.
Mnemonics are case-insensitive in the assembler.

## Instruction table

The assembler should map these mnemonics to the current VM opcode table.
If opcodes are added/removed, update this table and the assembler mapping.

All instructions are 1 word unless noted. For stack-based control flow, the
assembler accepts an operand and expands it to `PUSH <operand>` + `<op>`.

Stack ops:

- `PUSH <word>`         ; Push immediate program word (2 words total)
- `POP`                 ; Pop and discard
- `DUP`                 ; Duplicate top of stack
- `SWAP`                ; Swap top two values
- `RET <count>`         ; Return via saved frame pointer and return PC
- `SLOAD <offset>`      ; Push stack[frame_pointer + offset] (2 words total)
- `SSTORE <offset>`     ; Store top at stack[frame_pointer + offset] (2 words total)

Local ops:

- `LLOAD <offset>`      ; Push locals[MLP + offset] (2 words total)
- `LSTORE <offset>`     ; Pop -> locals[MLP + offset] (2 words total)

Global ops:

- `GLOAD <addr>`        ; Push globals[addr] (2 words total)
- `GSTORE <addr>`       ; Pop -> globals[addr] (2 words total)

Static ops:

- `LOAD_STATIC`         ; Pop addr, push static_data[addr]

Control flow:

- `JUMP`                ; Pop addr, jump to absolute word index
- `CALL`                ; Pop function index + arg count (stack: ... args, arg_count, func_index)
- `CALL_SHARED`         ; Pop shared function index + arg count (stack: ... args, arg_count, func_index)
- `BRLT`                ; Pop addr and compare a < b
- `BRLTE`               ; Pop addr and compare a <= b
- `BRGT`                ; Pop addr and compare a > b
- `BRGTE`               ; Pop addr and compare a >= b
- `BREQ`                ; Pop addr and compare a == b
- `EXIT`              ; Return from function

Logic ops (reserved):

- `AND` `OR` `XOR` `NOT` ; Logical ops on top-of-stack values
- `BAND` `BOR` `BXOR` `BNOT`   ; Bitwise forms

Arithmetic ops (reserved):

- `ADD` `SUB` `MUL` `DIV` `MOD` ; Arithmetic on top-of-stack values

## Semantics (current runtime)

Only these are executed today:

- `PUSH <word>`: push immediate.
- `POP`: pop top of stack, error if empty.
- `DUP`: duplicate top of stack, error if empty.
- `SWAP`: swap top two values, error if fewer than 2.
- `LLOAD <offset>`: read globals[MLP + offset], push; error if out of range.
- `LSTORE <offset>`: pop and store to globals[MLP + offset], error if empty/out of range.
- `GLOAD <addr>`: read globals[addr], push; error if addr out of range.
- `GSTORE <addr>`: pop and store to globals[addr], error if empty/out of range.
- `SLOAD <offset>`: push stack[frame_pointer + offset], error if out of range.
- `SSTORE <offset>`: store top into stack[frame_pointer + offset], error if out of range.
- `LOAD_STATIC`: pop addr, push static_data[addr].
- `JUMP`: pop addr and jump.
- `CALL`: pop function index + arg count (stack: ... args, arg_count, func_index), insert return PC and saved frame pointer before args, set frame pointer to first arg, call function, resume after.
- `CALL_SHARED`: pop shared function index + arg count and call a shared function using the same call frame semantics.
- `RET <count>`: copy `<count>` values from the top of the stack, remove the call frame,
  restore the saved frame pointer, push the copied values, and jump to the saved return PC.
- `BRLT`/`BRLTE`/`BRGT`/`BRGTE`/`BREQ`: pop addr and compare.
- `ADD`/`SUB`/`MUL`/`DIV`/`MOD`: pop two values, push arithmetic result.
- `EXIT`: return from function.

Reserved instructions assemble but are not executed yet. Programs using them
should be treated as "future programs" and may error at runtime.

## Program layout (informative)

The binary program is a `u16` (program word) array structured as:

    [machine_count][globals_size][machine_table...][machines...]

This document does not require authors to manually build the header; the
assembler should emit a valid header based on `.machine` and `.func` blocks.

## Example

This example mirrors the test program used in code:

    .machine main locals 3 functions 2

    .local red 0
    .local green 1
    .local blue 2

    .func set_rgb index 0
        LSTORE 0
        LSTORE 1
        LSTORE 2
        EXIT
    .end

    .func get_rgb index 1
        LLOAD 0
        LLOAD 1
        LLOAD 2
        EXIT
    .end

    .end

## Grammar (EBNF)

This is a minimal grammar suitable for a hand-written parser.
Whitespace and comments can appear between any tokens.

    program        = { item } ;
    item           = directive | instruction | label | data_word | empty ;
    empty          = ;

    directive      = machine_decl | shared_decl | local_decl | stack_decl | func_decl | func_forward_decl
                   | shared_func_decl | shared_func_forward_decl | data_decl | shared_data_decl | end_decl ;
    machine_decl   = ".machine" ident "locals" number "functions" number ;
    shared_decl    = ".shared" ident number ;
    local_decl     = ".local" ident number ;
    stack_decl     = ".frame" ident number ;
    func_decl      = ".func" ident [ "index" number ] ;
    func_forward_decl = ".func_decl" ident [ "index" number ] ;
    data_decl      = ".data" ident ;
    shared_func_decl = ".shared_func" ident [ "index" number ] ;
    shared_func_forward_decl = ".shared_func_decl" ident [ "index" number ] ;
    shared_data_decl = ".shared_data" ident ;
    end_decl       = ".end" ;

    label          = ident ":" ;

    instruction    = mnemonic [ operand ] ;
    data_word      = ".word" number | number ;
    operand        = number | ident ;

    mnemonic       = "PUSH" | "POP" | "DUP" | "SWAP" | "RET" | "SLOAD" | "SSTORE" | "LLOAD" | "LSTORE" | "GLOAD" | "GSTORE" | "LOAD_STATIC"
                   | "JUMP" | "CALL" | "BRLT" | "BRLTE" | "BRGT" | "BRGTE" | "BREQ"
                   | "EXIT"
                   | "AND" | "OR" | "XOR" | "NOT"
                   | "BAND" | "BOR" | "BXOR" | "BNOT"
                   | "ADD" | "SUB" | "MUL" | "DIV" | "MOD" ;

    number         = dec_number | hex_number ;
    dec_number     = digit { digit } ;
    hex_number     = "0x" hex_digit { hex_digit } ;

    ident          = ident_start { ident_cont } ;
    ident_start    = letter | "_" ;
    ident_cont     = letter | digit | "_" ;

    digit          = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
    hex_digit      = digit | "a" | "b" | "c" | "d" | "e" | "f"
                   | "A" | "B" | "C" | "D" | "E" | "F" ;
    letter         = "A" ... "Z" | "a" ... "z" ;

## Parsing

- The parser should track the current machine/function/data block.
- A `.func_decl` defines the function name/index without emitting code.
- A `.func` must either define a new function or provide the body for a previous
  `.func_decl`. Multiple bodies for the same name are an error.
- Labels resolve to word indices within the current block.
- `operand` label references resolve to the word index of the label in the same
  function or data block, to a named local declared with `.local` (for LLOAD/LSTORE),
  to a named shared global declared with `.shared` (for GLOAD/GSTORE), or to a named
  stack slot declared with `.frame` (for SLOAD/SSTORE).
- `.end` closes the most recent open block (function/data first, then machine).
- Instructions are only valid inside `.func` blocks; `.data` blocks accept only
  `.word` or bare numeric values.

## Future extensions (placeholders)

- `.const` for named single-word constants.
- `.include` for file inclusion.
- `.assert` for assembly-time checks.
- Named machine indices (auto-generated IDs for inter-machine calls).
