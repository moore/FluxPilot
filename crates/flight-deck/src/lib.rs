use wasm_bindgen::prelude::*;

use pliot::protocol::{Controler, ErrorType, FunctionId, MessageType, Protocol};

use light_machine::{
    ProgramDescriptor,
    Word,
    assembler::{Assembler, AssemblerError, AssemblerErrorKind},
    builder::*,
};
use postcard::{to_vec_cobs, from_bytes_cobs};

use heapless::Vec;
use std::vec::Vec as StdVec;

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;
const ASM_MACHINE_MAX: usize = 8;
const ASM_FUNCTION_MAX: usize = 32;
const ASM_LABEL_CAP: usize = 64;
const ASM_DATA_CAP: usize = 256;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

#[wasm_bindgen]
pub enum FlightDeckError {
    ToManyArguments,
    CouldNotReceive,
    InvalidProgramLength,
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
    pub fn on_return(this: &ReceiveHandler, request_id: u64, result: &[Word]);

    #[wasm_bindgen(method, js_name = onNotification)]
    pub fn on_notification(
        this: &ReceiveHandler,
        machine_index: Word,
        function_index: u32,
        result: &[Word],
    );

    #[wasm_bindgen(method, js_name = onError)]
    pub fn on_error(
        this: &ReceiveHandler,
        has_request_id: bool,
        request_id: u64,
        error_code: u32,
        error_message: &str,
    );
}



#[wasm_bindgen]
pub struct FlightDeck {
    controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>,
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
            .machines
            .iter()
            .map(|machine| machine.functions.len() as u32)
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
        FlightDeck {
            controler: Controler::new(),
        }
    }

    pub fn call(&mut self, machine_index: Word, function_index: u32, args: &[Word]) -> Result<Option<u64>, FlightDeckError> {
        if args.len() > MAX_ARGS {
            return Err(FlightDeckError::ToManyArguments);
        }

        let mut call_args = Vec::<Word, MAX_ARGS>::new();

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

        let message_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();
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
            Protocol::Error { request_id, error_type } => {
                let (has_request_id, request_id_value) = match request_id {
                    Some(id) => (true, id.value()),
                    None => (false, 0),
                };
                let message = error_message(&error_type);
                handler.on_error(
                    has_request_id,
                    request_id_value,
                    error_code(&error_type),
                    &message,
                );
            }
            _ => {
            }
        }

        Ok(())   
    }

    pub fn load_program(&mut self, program: &[Word], length: usize) -> Result<(), FlightDeckError> {
        if length > program.len() {
            return Err(FlightDeckError::InvalidProgramLength);
        }

        let program = &program[..length];
        let loader = self.controler.get_program_loader(program);
        for message in loader {
            let message_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();
            send(message_buf.as_slice());
        }

        Ok(())
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
        ErrorType::UnalignedWrite => 8,
        ErrorType::WriteFailed => 9,
        ErrorType::InvalidHeader => 10,
        ErrorType::InvalidProgram => 11,
        ErrorType::UnknownProgram => 12,
    }
}

fn error_message(error_type: &ErrorType) -> String {
    match error_type {
        ErrorType::UnknownResuestId(_) => "unknown request id".to_string(),
        ErrorType::UnexpectedProgramBlock(_) => "unexpected program block".to_string(),
        ErrorType::UnknownMachine(_) => "unknown machine".to_string(),
        ErrorType::UnknownFucntion(_) => "unknown function".to_string(),
        ErrorType::UnexpectedMessageType(message_type) => {
            format!("unexpected message type {}", message_type_name(message_type))
        }
        ErrorType::InvalidMessage => "invalid message (too large or corrupted)".to_string(),
        ErrorType::ProgramTooLarge => "program too large".to_string(),
        ErrorType::UnalignedWrite => "unaligned flash write".to_string(),
        ErrorType::WriteFailed => "flash write failed".to_string(),
        ErrorType::InvalidHeader => "invalid flash header".to_string(),
        ErrorType::InvalidProgram => "invalid program".to_string(),
        ErrorType::UnknownProgram => "unknown program".to_string(),
    }
}

const fn message_type_name(message_type: &MessageType) -> &'static str {
    match message_type {
        MessageType::Call => "Call",
        MessageType::Return => "Return",
        MessageType::Notifacation => "Notifacation",
        MessageType::Error => "Error",
        MessageType::LoadProgram => "LoadProgram",
        MessageType::ProgramBlock => "ProgramBlock",
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
    let machine_count = count_machines(source)?;
    let builder = ProgramBuilder::<ASM_MACHINE_MAX, ASM_FUNCTION_MAX>::new(buffer, machine_count)
        .map_err(|_| JsValue::from_str("program buffer too small for machine count"))?;
    let mut assembler: Assembler<
        ASM_MACHINE_MAX,
        ASM_FUNCTION_MAX,
        ASM_LABEL_CAP,
        ASM_DATA_CAP,
    > = Assembler::new(builder);

    for line in source.lines() {
        assembler.add_line(line).map_err(assembler_error_to_js)?;
    }

    let descriptor = assembler.finish().map_err(assembler_error_to_js)?;
    Ok(ProgramDescriptorJs::from_descriptor(descriptor))
}

fn build_test_program(buffer: &mut [u16]) -> Result<ProgramDescriptor<1, 2>, JsValue> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let program_builder =
        ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(buffer, MACHINE_COUNT as u16)
            .map_err(|_| JsValue::from_str("could not get machine builder"))?;

    let globals_size = 3;
    let machine = program_builder
        .new_machine(FUNCTION_COUNT as u16, globals_size)
        .map_err(|_| JsValue::from_str("could not get new machine"))?;

    let mut function = machine
        .new_function()
        .map_err(|_| JsValue::from_str("could not get function builder"))?;
    function
        .add_op(Op::Store(0))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Store(1))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Store(2))
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
        .add_op(Op::Load(0))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Load(1))
        .map_err(|_| JsValue::from_str("could not add op"))?;
    function
        .add_op(Op::Load(2))
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

fn count_machines(source: &str) -> Result<u16, JsValue> {
    let mut count: u16 = 0;
    for line in source.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with(".machine") {
            count = count
                .checked_add(1)
                .ok_or_else(|| JsValue::from_str("machine count overflow"))?;
        }
    }
    if count == 0 {
        return Err(JsValue::from_str("no .machine directive found"));
    }
    Ok(count)
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
