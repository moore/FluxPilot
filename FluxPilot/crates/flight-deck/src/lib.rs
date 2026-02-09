use wasm_bindgen::prelude::*;

use console_error_panic_hook;
use pliot::protocol::{Controler, ErrorType, FunctionId, MessageType, Protocol};

use light_machine::{
    ProgramDescriptor,
    ProgramWord,
    StackWord,
    assembler::{AssemblerError, AssemblerErrorKind},
    builder::*,
};
use postcard::{to_vec_cobs, from_bytes_cobs};

use heapless::Vec;
use std::vec::Vec as StdVec;

mod graph_assembler;
mod program_graph;

use graph_assembler::GraphAssembler;

const MAX_ARGS: usize = 10;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 64;
const UI_BLOCK_SIZE: usize = 128;
const ASM_MACHINE_MAX: usize = 256;
const ASM_FUNCTION_MAX: usize = 256;
const SHARED_FUNCTION_RESERVED_COUNT: u16 = 4;
const I2C_MAPPING_GLOBALS_WORDS: u16 = 64;
const I2C_DEFAULTS_BLOCK: &str = "i2c_defaults";
const I2C_DEFAULT_LABEL_PREFIX: &str = "i2c_default_";
const I2C_INIT_SHARED_FUNCTION_NAME: &str = "init_program";
const I2C_INIT_SHARED_FUNCTION_INDEX: u16 = 0;
const I2C_SHARED_GLOBAL_ANCHOR: &str = "__i2c_map_last__";

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>;

#[wasm_bindgen]
pub enum FlightDeckError {
    ToManyArguments,
    CouldNotReceive,
    InvalidProgramLength,
    CouldNotEncode,
}


#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    pub fn console_log(s: &str);
}

#[wasm_bindgen(module = "/deck.js")]
extern "C" {
    pub fn send(bytes: &[u8]);
}

#[wasm_bindgen(module = "/deck.js")]
extern "C" {
    #[wasm_bindgen(js_name = ReceiveHandler)]
    pub type ReceiveHandler;

    #[wasm_bindgen(method, js_name = onReturn)]
    pub fn on_return(this: &ReceiveHandler, request_id: u64, result: &[StackWord]);

    #[wasm_bindgen(method, js_name = onNotification)]
    pub fn on_notification(
        this: &ReceiveHandler,
        machine_index: ProgramWord,
        function_index: u32,
        result: &[StackWord],
    );

    #[wasm_bindgen(method, js_name = onError)]
    pub fn on_error(
        this: &ReceiveHandler,
        has_request_id: bool,
        request_id: u64,
        error_code: u32,
        error_message: &str,
    );

    #[wasm_bindgen(method, js_name = onUiStateBlock)]
    pub fn on_ui_state_block(
        this: &ReceiveHandler,
        request_id: u64,
        total_size: u32,
        block_number: u32,
        block: &[u8],
    );
}



#[wasm_bindgen]
pub struct FlightDeck {
    controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>,
}

#[wasm_bindgen]
pub struct ProgramDescriptorJs {
    length: usize,
    machine_function_counts: StdVec<u32>,
}

impl ProgramDescriptorJs {
    fn from_descriptor<const MACHINE_COUNT: usize, const FUNCTION_COUNT: usize>(
        descriptor: ProgramDescriptor<MACHINE_COUNT, FUNCTION_COUNT>,
    ) -> Self {
        let machine_function_counts = descriptor
            .instances
            .iter()
            .map(|instance| {
                descriptor
                    .types
                    .get(instance.type_id as usize)
                    .map(|machine| machine.functions.len() as u32)
                    .unwrap_or(0)
            })
            .collect();
        Self {
            length: descriptor.length,
            machine_function_counts,
        }
    }
}

#[wasm_bindgen]
impl ProgramDescriptorJs {
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.length
    }

    pub fn machine_count(&self) -> u32 {
        self.machine_function_counts.len() as u32
    }

    pub fn function_counts(&self) -> StdVec<u32> {
        self.machine_function_counts.clone()
    }

    pub fn function_count(&self, machine_index: usize) -> Result<u32, JsValue> {
        self.machine_function_counts
            .get(machine_index)
            .copied()
            .ok_or_else(|| JsValue::from_str("machine index out of range"))
    }
}

impl Default for FlightDeck {
    fn default() -> Self {
        Self::new()
    }
}


#[wasm_bindgen]
impl FlightDeck {
    #[wasm_bindgen(constructor)]
    pub fn new() -> FlightDeck {
        console_error_panic_hook::set_once();
        FlightDeck {
            controler: Controler::new(),
        }
    }

    pub fn call(
        &mut self,
        machine_index: ProgramWord,
        function_index: u32,
        args: &[StackWord],
    ) -> Result<Option<u64>, FlightDeckError> {
        if args.len() > MAX_ARGS {
            return Err(FlightDeckError::ToManyArguments);
        }

        let mut call_args = Vec::<StackWord, MAX_ARGS>::new();

        for arg in args {
           if call_args.push(*arg).is_err() {
            unreachable!("The check abouve makes this unreachable");
           }
        }

        let function = FunctionId {
            machine_index,
            function_index,
        };

        let message = self.controler.call(function, call_args);

        let message_buf = to_vec_cobs::<ProtocolType, 512>(&message)
            .map_err(|_| FlightDeckError::CouldNotEncode)?;
        let bytes = message_buf.as_slice();
        send(bytes);

        let request_id = message.get_request_id().map(|id| id.value());

        Ok(request_id)
    }

    pub fn receive(
        &mut self,
        data: &mut [u8],
        handler: &ReceiveHandler,
    ) -> Result<(), FlightDeckError> {
        let message: ProtocolType =
            from_bytes_cobs(data).map_err(|_| FlightDeckError::CouldNotReceive)?;

        match message {
            Protocol::Return { request_id, result } => {
                handler.on_return(request_id.value(), result.as_slice());
            }
            Protocol::Notifacation { function, result } => {
                handler.on_notification(
                    function.machine_index,
                    function.function_index,
                    result.as_slice(),
                );
            }
            Protocol::Error {
                request_id,
                error_type,
                location,
            } => {
                let (has_request_id, request_id_value) = match request_id {
                    Some(id) => (true, id.value()),
                    None => (false, 0),
                };
                let message = error_message(&error_type, location.as_ref());
                handler.on_error(
                    has_request_id,
                    request_id_value,
                    error_code(&error_type),
                    &message,
                );
            }
            Protocol::UiStateBlock {
                request_id,
                total_size,
                block_number,
                block,
            } => {
                handler.on_ui_state_block(
                    request_id.value(),
                    total_size,
                    block_number,
                    block.as_slice(),
                );
            }
            _ => {
            }
        }

        Ok(())   
    }

    pub fn load_program(
        &mut self,
        program: &[ProgramWord],
        length: usize,
        ui_state: &[u8],
    ) -> Result<(), FlightDeckError> {
        if length > program.len() {
            return Err(FlightDeckError::InvalidProgramLength);
        }

        let program = &program[..length];
        let loader = self.controler.get_program_loader(program, ui_state);
        for message in loader {
            let message_buf = to_vec_cobs::<ProtocolType, 512>(&message)
                .map_err(|_| FlightDeckError::CouldNotEncode)?;
            send(message_buf.as_slice());
        }

        Ok(())
    }

    pub fn read_ui_state_block(&mut self, block_number: u32) -> Result<Option<u64>, FlightDeckError> {
        let message = self.controler.read_ui_state(block_number);
        let message_buf = to_vec_cobs::<ProtocolType, 512>(&message)
            .map_err(|_| FlightDeckError::CouldNotEncode)?;
        send(message_buf.as_slice());
        let request_id = message.get_request_id().map(|id| id.value());
        Ok(request_id)
    }
}

const fn error_code(error_type: &ErrorType) -> u32 {
    match error_type {
        ErrorType::UnknownResuestId(_) => 1,
        ErrorType::UnexpectedProgramBlock(_) => 2,
        ErrorType::UnknownMachine(_) => 3,
        ErrorType::UnknownFucntion(_) => 4,
        ErrorType::UnexpectedMessageType(_) => 5,
        ErrorType::InvalidMessage => 6,
        ErrorType::ProgramTooLarge => 7,
        ErrorType::ProgramIncomplete => 8,
        ErrorType::UnalignedWrite => 9,
        ErrorType::WriteFailed => 10,
        ErrorType::InvalidHeader => 11,
        ErrorType::InvalidProgram => 12,
        ErrorType::UnknownProgram => 13,
        ErrorType::UiStateTooLarge => 14,
        ErrorType::UiStateIncomplete => 15,
        ErrorType::UiStateReadOutOfBounds => 16,
    }
}

fn error_message(
    error_type: &ErrorType,
    location: Option<&pliot::protocol::ErrorLocation>,
) -> String {
    let base = match error_type {
        ErrorType::UnknownResuestId(_) => "unknown request id".to_string(),
        ErrorType::UnexpectedProgramBlock(_) => "unexpected program block".to_string(),
        ErrorType::UnknownMachine(_) => "unknown machine".to_string(),
        ErrorType::UnknownFucntion(_) => "unknown function".to_string(),
        ErrorType::UnexpectedMessageType(message_type) => {
            format!("unexpected message type {}", message_type_name(message_type))
        }
        ErrorType::InvalidMessage => "invalid message (too large or corrupted)".to_string(),
        ErrorType::ProgramTooLarge => "program too large".to_string(),
        ErrorType::ProgramIncomplete => "program incomplete".to_string(),
        ErrorType::UnalignedWrite => "unaligned flash write".to_string(),
        ErrorType::WriteFailed => "flash write failed".to_string(),
        ErrorType::InvalidHeader => "invalid flash header".to_string(),
        ErrorType::InvalidProgram => "invalid program".to_string(),
        ErrorType::UnknownProgram => "unknown program".to_string(),
        ErrorType::UiStateTooLarge => "ui state too large".to_string(),
        ErrorType::UiStateIncomplete => "ui state incomplete".to_string(),
        ErrorType::UiStateReadOutOfBounds => "ui state read out of bounds".to_string(),
    };

    match location {
        Some(location) => format!(
            "{} (at {}:{}:{})",
            base,
            location.file.as_str(),
            location.line,
            location.column
        ),
        None => base,
    }
}

const fn message_type_name(message_type: &MessageType) -> &'static str {
    match message_type {
        MessageType::Call => "Call",
        MessageType::Return => "Return",
        MessageType::Notifacation => "Notifacation",
        MessageType::Error => "Error",
        MessageType::CallStaticFunction => "CallStaticFunction",
        MessageType::StaticFunctionResult => "StaticFunctionResult",
        MessageType::LoadProgram => "LoadProgram",
        MessageType::ProgramBlock => "ProgramBlock",
        MessageType::UiStateBlock => "UiStateBlock",
        MessageType::ReadUiState => "ReadUiState",
        MessageType::FinishProgram => "FinishProgram",
    }
}

#[wasm_bindgen]
pub fn get_test_program(buffer: &mut [u16]) -> Result<ProgramDescriptorJs, JsValue> {
    let descriptor = build_test_program(buffer)?;
    Ok(ProgramDescriptorJs::from_descriptor(descriptor))
}

#[wasm_bindgen]
pub fn compile_program(source: &str, buffer: &mut [u16]) -> Result<ProgramDescriptorJs, JsValue> {
    let injected_source = inject_i2c_init_program(source)?;
    console_log(injected_source.as_str());
    let shared_function_count = count_shared_functions(&injected_source)?;
    let mut assembler = GraphAssembler::new(shared_function_count);
    for line in injected_source.lines() {
        assembler.add_line(line).map_err(assembler_error_to_js)?;
    }
    let graph = assembler.finish().map_err(assembler_error_to_js)?;
    if graph.instance_count() == 0 {
        return Err(JsValue::from_str("no .machine directive found"));
    }
    let builder = ProgramBuilder::<ASM_MACHINE_MAX, ASM_FUNCTION_MAX>::new(
        buffer,
        graph.instance_count(),
        graph.type_count(),
        graph.shared_function_count(),
    )
    .map_err(|_| JsValue::from_str("program buffer too small for machine count"))?;
    let descriptor = graph
        .emit_into(builder)
        .map_err(|_| JsValue::from_str("program builder error"))?;
    Ok(ProgramDescriptorJs::from_descriptor(descriptor))
}

fn build_test_program(buffer: &mut [u16]) -> Result<ProgramDescriptor<1, 2>, JsValue> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let program_builder = ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(
        buffer,
        MACHINE_COUNT as u16,
        MACHINE_COUNT as u16,
        SHARED_FUNCTION_RESERVED_COUNT,
    )
    .map_err(|_| JsValue::from_str("could not get machine builder"))?;

    let globals_size = 3;
    let mut program_builder = program_builder;
    for index in 0..SHARED_FUNCTION_RESERVED_COUNT {
        let mut function = program_builder
            .new_shared_function_at_index(FunctionIndex::new(index))
            .map_err(|_| JsValue::from_str("could not get shared function builder"))?;
        function
            .add_op(Op::Exit)
            .map_err(|_| JsValue::from_str("could not add op"))?;
        let (_index, next_program) = function
            .finish()
            .map_err(|_| JsValue::from_str("could not finish shared function"))?;
        program_builder = next_program;
    }

    let machine = program_builder
        .new_machine(FUNCTION_COUNT as u16, globals_size)
        .map_err(|_| JsValue::from_str("could not get new machine"))?;

    let mut function = machine
        .new_function()
        .map_err(|_| JsValue::from_str("could not get function builder"))?;
    function
        .add_op(Op::LocalStore(0))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::LocalStore(1))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::LocalStore(2))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Exit)
        .map_err(|_| JsValue::from_str("could not add op"))?;
    let (_store_function_index, machine) = function
        .finish()
        .map_err(|_| JsValue::from_str("could not finish function"))?;

    let mut function = machine
        .new_function()
        .map_err(|_| JsValue::from_str("could not get function builder"))?;
    function
        .add_op(Op::LocalLoad(0))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::LocalLoad(1))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::LocalLoad(2))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Exit)
        .map_err(|_| JsValue::from_str("could not add op"))?;
    let (_get_function_index, machine) = function
        .finish()
        .map_err(|_| JsValue::from_str("could not finish function"))?;

    let program_builder =
        machine.finish().map_err(|_| JsValue::from_str("could not finish machine"))?;

    let descriptor = program_builder.finish_program();

    Ok(descriptor)
}

fn count_shared_functions(source: &str) -> Result<u16, JsValue> {
    let mut max_index: u16 = 0;
    let mut has_shared = false;
    let mut next_auto: u16 = 0;
    for line in source.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if !line.starts_with(".shared_func") && !line.starts_with(".shared_func_decl") {
            continue;
        }
        has_shared = true;
        let mut tokens = line.split_whitespace();
        let _ = tokens.next(); // directive
        let _ = tokens.next(); // name
        if tokens.next() == Some("index") {
            let token = tokens
                .next()
                .ok_or_else(|| JsValue::from_str("invalid shared function index"))?;
            let index: u16 = token
                .parse()
                .map_err(|_| JsValue::from_str("invalid shared function index"))?;
            if index > max_index {
                max_index = index;
            }
        } else {
            if next_auto > max_index {
                max_index = next_auto;
            }
            next_auto = next_auto
                .checked_add(1)
                .ok_or_else(|| JsValue::from_str("shared function count overflow"))?;
        }
    }
    if !has_shared {
        return Ok(SHARED_FUNCTION_RESERVED_COUNT);
    }
    let count = max_index
        .checked_add(1)
        .ok_or_else(|| JsValue::from_str("shared function count overflow"))?;
    Ok(count.max(SHARED_FUNCTION_RESERVED_COUNT))
}

fn inject_i2c_init_program(source: &str) -> Result<String, JsValue> {
    if I2C_MAPPING_GLOBALS_WORDS == 0 {
        return Ok(source.to_string());
    }
    if shared_function_index_defined(source, I2C_INIT_SHARED_FUNCTION_INDEX)? {
        return Ok(source.to_string());
    }
    let has_defaults = has_shared_data_block(source, I2C_DEFAULTS_BLOCK);
    let injection = build_i2c_injection(has_defaults)?;
    Ok(insert_program_prelude(source, &injection))
}

fn shared_function_index_defined(source: &str, target_index: u16) -> Result<bool, JsValue> {
    use std::collections::HashSet;
    let mut used: HashSet<u16> = HashSet::new();
    let mut next_auto: u16 = 0;
    let mut has_shared = false;
    for line in source.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if !line.starts_with(".shared_func") && !line.starts_with(".shared_func_decl") {
            continue;
        }
        has_shared = true;
        let mut tokens = line.split_whitespace();
        let _ = tokens.next(); // directive
        let _ = tokens.next(); // name
        let mut index: Option<u16> = None;
        if tokens.next() == Some("index") {
            let token = tokens
                .next()
                .ok_or_else(|| JsValue::from_str("invalid shared function index"))?;
            index = Some(
                token
                    .parse()
                    .map_err(|_| JsValue::from_str("invalid shared function index"))?,
            );
        }
        let assigned = if let Some(value) = index {
            value
        } else {
            while used.contains(&next_auto) {
                next_auto = next_auto
                    .checked_add(1)
                    .ok_or_else(|| JsValue::from_str("shared function count overflow"))?;
            }
            let value = next_auto;
            next_auto = next_auto
                .checked_add(1)
                .ok_or_else(|| JsValue::from_str("shared function count overflow"))?;
            value
        };
        used.insert(assigned);
        if assigned == target_index {
            return Ok(true);
        }
    }
    Ok(has_shared && used.contains(&target_index))
}

fn has_shared_data_block(source: &str, block_name: &str) -> bool {
    for line in source.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if !line.starts_with(".shared_data") {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let _ = tokens.next();
        if tokens.next() == Some(block_name) {
            return true;
        }
    }
    false
}

fn build_i2c_injection(has_defaults: bool) -> Result<String, JsValue> {
    let mut lines: StdVec<String> = StdVec::new();
    let last_index = I2C_MAPPING_GLOBALS_WORDS
        .checked_sub(1)
        .ok_or_else(|| JsValue::from_str("invalid i2c mapping size"))?;
    lines.push(format!(".shared {} {}", I2C_SHARED_GLOBAL_ANCHOR, last_index));
    if !has_defaults {
        lines.push(format!(".shared_data {}", I2C_DEFAULTS_BLOCK));
        for index in 0..I2C_MAPPING_GLOBALS_WORDS {
            lines.push(format!("    {}{}:", I2C_DEFAULT_LABEL_PREFIX, index));
            lines.push("    .word 0".to_string());
        }
        lines.push(".end".to_string());
    }
    lines.push(format!(
        ".shared_func {} index {}",
        I2C_INIT_SHARED_FUNCTION_NAME, I2C_INIT_SHARED_FUNCTION_INDEX
    ));
    for index in 0..I2C_MAPPING_GLOBALS_WORDS {
        lines.push(format!(
            "    LOAD_STATIC {}{}",
            I2C_DEFAULT_LABEL_PREFIX, index
        ));
        lines.push(format!("    GSTORE {}", index));
    }
    lines.push("    EXIT".to_string());
    lines.push(".end".to_string());
    Ok(lines.join("\n"))
}

fn insert_program_prelude(source: &str, injection: &str) -> String {
    let mut lines: StdVec<&str> = source.lines().collect();
    let mut insert_at = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.split(';').next().unwrap_or("").trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with(".shared ") || trimmed.starts_with(".shared\t") {
            insert_at = idx + 1;
            continue;
        }
        break;
    }
    if insert_at >= lines.len() {
        return format!("{}\n{}", source.trim_end(), injection);
    }
    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx == insert_at {
            out.push_str(injection);
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn assembler_error_to_js(err: AssemblerError) -> JsValue {
    let kind = match err.error_kind() {
        AssemblerErrorKind::EmptyLine => "empty line",
        AssemblerErrorKind::TooManyTokens => "too many tokens",
        AssemblerErrorKind::InvalidDirective => "invalid directive",
        AssemblerErrorKind::InvalidInstruction => "invalid instruction",
        AssemblerErrorKind::InvalidNumber => "invalid number",
        AssemblerErrorKind::NameTooLong => "name too long",
        AssemblerErrorKind::DuplicateLabel => "duplicate label",
        AssemblerErrorKind::DuplicateGlobal => "duplicate global",
        AssemblerErrorKind::DuplicateStackSlot => "duplicate stack slot",
        AssemblerErrorKind::GlobalIndexOutOfRange => "global index out of range",
        AssemblerErrorKind::MaxLabelsExceeded => "max labels exceeded",
        AssemblerErrorKind::UnknownLabel => "unknown label",
        AssemblerErrorKind::MissingMachine => "missing machine",
        AssemblerErrorKind::MissingFunction => "missing function",
        AssemblerErrorKind::MissingProgram => "missing program",
        AssemblerErrorKind::UnexpectedDirective => "unexpected directive",
        AssemblerErrorKind::UnexpectedInstruction => "unexpected instruction",
        AssemblerErrorKind::FunctionAlreadyDefined => "function already defined",
        AssemblerErrorKind::FunctionNotDeclared => "function not declared",
        AssemblerErrorKind::FunctionIndexOutOfRange => "function index out of range",
        AssemblerErrorKind::FunctionIndexDuplicate => "function index duplicate",
        AssemblerErrorKind::MaxFunctionsExceeded => "max functions exceeded",
        AssemblerErrorKind::LineNumberOverflow => "line number overflow",
        AssemblerErrorKind::CursorOverflow => "cursor overflow",
        AssemblerErrorKind::DataTooLarge => "data too large",
        AssemblerErrorKind::Builder(_) => "builder error",
    };
    match err.line_number() {
        Some(line) => JsValue::from_str(&format!("line {}: {}", line, kind)),
        None => JsValue::from_str(kind),
    }
}
