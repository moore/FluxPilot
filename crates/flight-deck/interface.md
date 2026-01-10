# Flight Deck Interface Spec

This document defines the intended interface and behavior for the new
`index.html` in `crates/flight-deck/`.

## Goals

- Provide a clean, fast single-page interface for viewing and controlling
  Flight Deck content.
- Support both desktop and mobile layouts without sacrificing clarity.
- Minimize dependencies; keep the page portable and easy to embed.
- The software should be based on web components and not use any frameworks such as
  React or Angular.

## Overall concept

The UI will be inspired by music and animation software. The core concept is that
the program will be represented by a list of machines (see `crates/light_machine/design.md`).

Definitions:

- Machine: a compiled `light_machine` program instance with one or more functions
  (see `crates/light_machine/design.md`).
- Function: a callable VM entry point that may compute LED values from inputs
  (see `crates/light_machine/design.md`).
- Track: a UI lane that references a machine and presents its parameters,
  similar to a tracker row or pattern.

User model:

- Users select a machine for each track.
- Each track represents one machine in the UI, even though the machine inputs
  (tick and LED index) are not exposed directly.

Settings:

- Track settings apply to the selected machine (e.g., speed, color).
- Settings map to machine inputs or globals defined by the assembler program
  (see `crates/light_machine/language.md` for program construction).
