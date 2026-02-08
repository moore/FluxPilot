# I2C UI Integration Specification (pliot)

## Status

Draft.

## Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD",
"SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" are to be interpreted as
described in RFC 2119 and RFC 8174.

## Overview

This document specifies the UI-side requirements for I2C mapping and routing
when interacting with the firmware I2C integration.

## Design Rationale (Non-Normative)

The UI owns the user-facing mapping experience. It observes I2C traffic in
mapping mode and binds devices to VM targets without requiring firmware changes.

Static function calls are the mechanism for updating routing because routing
state lives inside the VM program. This keeps device behavior synchronized with
the loaded program and avoids hidden firmware configuration.

Error handling on static function results is critical for user feedback and for
keeping the UI in sync with the device state.

## Requirements

### Mapping Mode Behavior

- I2C-UI-REQ-001: The UI MUST support a mapping mode that listens for
  `I2C_EVENT` messages from the device.
- I2C-UI-REQ-002: The UI MUST allow users to bind observed events (by
  `bus_id` and `address_7bit`) to VM targets `(machine_id, function_id)`.

### Static Function Calls

- I2C-UI-REQ-010: The UI MUST call static function id `1` (`get_routes`) to
  load or refresh the routing table.
- I2C-UI-REQ-011: The UI MUST call static function id `2` (`add_route`) to add
  a mapping, passing `bus_id`, `address_7bit`, `machine_id`, `function_id` on
  the stack.
- I2C-UI-REQ-012: The UI MUST call static function id `3` (`remove_route`) to
  remove a mapping, passing `bus_id`, `address_7bit`, `machine_id`,
  `function_id` on the stack.

### Protocol Handling

- I2C-UI-REQ-020: The UI MUST send `CALL_STATIC_FUNCTION` messages without a
  machine id.
- I2C-UI-REQ-021: The UI MUST handle `STATIC_FUNCTION_RESULT` responses and
  use the request id to correlate with the original call.
- I2C-UI-REQ-022: The UI MUST treat `STATIC_FUNCTION_RESULT` error codes as
  failures to be surfaced to the user.
