use core::cmp::min;

use heapless::{String, Vec};
use light_machine::{ProgramWord, StackWord};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(u64);

impl RequestId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

#[derive(Serialize, Deserialize)]
pub struct MachineId(u32);

#[derive(Serialize, Deserialize, Debug)]
pub struct FunctionId {
    pub machine_index: ProgramWord,
    pub function_index: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageType {
    Call,
    Return,
    Notifacation,
    Error,
    LoadProgram,
    ProgramBlock,
    UiStateBlock,
    ReadUiState,
    FinishProgram,
}

pub const ERROR_LOCATION_FILE_MAX: usize = 96;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ErrorLocation {
    pub file: String<ERROR_LOCATION_FILE_MAX>,
    pub line: u32,
    pub column: u32,
}

impl ErrorLocation {
    #[track_caller]
    pub fn capture() -> Self {
        let loc = core::panic::Location::caller();
        Self::from_parts(loc.file(), loc.line(), loc.column())
    }

    pub fn from_parts(file: &str, line: u32, column: u32) -> Self {
        let mut file_name = String::new();
        for ch in file.chars() {
            if file_name.push(ch).is_err() {
                break;
            }
        }
        Self {
            file: file_name,
            line,
            column,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ErrorType {
    UnknownResuestId(RequestId),
    UnexpectedProgramBlock(u32),
    UnknownMachine(u32),
    UnknownFucntion(u32),
    UnexpectedMessageType(MessageType),
    InvalidMessage,
    ProgramTooLarge,
    ProgramIncomplete,
    UnalignedWrite,
    WriteFailed,
    InvalidHeader,
    InvalidProgram,
    UnknownProgram,
    UiStateTooLarge,
    UiStateIncomplete,
    UiStateReadOutOfBounds,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Protocol<
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
> {
    /// Call a function on a machine
    Call {
        request_id: RequestId,
        function: FunctionId,
        args: Vec<StackWord, MAX_ARGS>,
    },
    /// The return value of a call to a function
    Return {
        request_id: RequestId,
        result: Vec<StackWord, MAX_RESULT>,
    },
    /// Notification that a function was called on a machine
    Notifacation {
        function: FunctionId,
        result: Vec<StackWord, MAX_RESULT>,
    },
    /// Function call produced error.
    Error {
        request_id: Option<RequestId>,
        error_type: ErrorType,
        location: Option<ErrorLocation>,
    },
    /// Start new program load
    LoadProgram {
        request_id: RequestId,
        size: u32,
        ui_state_size: u32,
        block_number: u32,
        block: Vec<ProgramWord, PROGRAM_BLOCK_SIZE>,
    },
    /// Program Block
    ProgramBlock {
        request_id: RequestId,
        block_number: u32,
        block: Vec<ProgramWord, PROGRAM_BLOCK_SIZE>,
    },
    /// UI state block (opaque compressed bytes)
    UiStateBlock {
        request_id: RequestId,
        total_size: u32,
        block_number: u32,
        block: Vec<u8, UI_BLOCK_SIZE>,
    },
    /// Read UI state block
    ReadUiState {
        request_id: RequestId,
        block_number: u32,
    },
    /// Finish the new program load
    FinishProgram { request_id: RequestId },
}

impl<
        const MAX_ARGS: usize,
        const MAX_RESULT: usize,
        const PROGRAM_BLOCK_SIZE: usize,
        const UI_BLOCK_SIZE: usize,
    > Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>
{
    pub fn get_request_id(&self) -> Option<RequestId> {
        match self {
            Protocol::Call { request_id, .. } => Some(*request_id),
            Protocol::Return { request_id, .. } => Some(*request_id),
            Protocol::Notifacation { .. } => None,
            Protocol::Error { request_id, .. } => *request_id,
            Protocol::LoadProgram { request_id, .. } => Some(*request_id),
            Protocol::ProgramBlock { request_id, .. } => Some(*request_id),
            Protocol::UiStateBlock { request_id, .. } => Some(*request_id),
            Protocol::ReadUiState { request_id, .. } => Some(*request_id),
            Protocol::FinishProgram { request_id, .. } => Some(*request_id),
        }
    }
}

pub struct Controler<
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
> {
    next_request: u64,
}

impl<
        const MAX_ARGS: usize,
        const MAX_RESULT: usize,
        const PROGRAM_BLOCK_SIZE: usize,
        const UI_BLOCK_SIZE: usize,
    > Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>
{
    pub fn new() -> Self {
        Controler { next_request: 0 }
    }

    pub fn call(
        &mut self,
        function: FunctionId,
        args: Vec<StackWord, MAX_ARGS>,
    ) -> Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> {
        let request_id = self.get_request_id();
        Protocol::Call {
            request_id,
            function,
            args,
        }
    }

    pub fn get_program_loader<'a>(
        &mut self,
        program: &'a [ProgramWord],
        ui_state: &'a [u8],
    ) -> ProgramLoader<'a, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> {
        let request_id = self.get_request_id();
        ProgramLoader::new(request_id, program, ui_state)
    }

    pub fn read_ui_state(
        &mut self,
        block_number: u32,
    ) -> Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> {
        let request_id = self.get_request_id();
        Protocol::ReadUiState {
            request_id,
            block_number,
        }
    }

    fn get_request_id(&mut self) -> RequestId {
        self.next_request = self.next_request.wrapping_add(1);
        RequestId(self.next_request)
    }
}

impl<
        const MAX_ARGS: usize,
        const MAX_RESULT: usize,
        const PROGRAM_BLOCK_SIZE: usize,
        const UI_BLOCK_SIZE: usize,
    > Default for Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>
{
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProgramLoader<
    'a,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
> {
    request_id: RequestId,
    next_program_block: u32,
    next_ui_block: u32,
    next_offset: usize,
    program: &'a [ProgramWord],
    ui_state: &'a [u8],
    ui_offset: usize,
    finished: bool,
}

impl<
        'a,
        const MAX_ARGS: usize,
        const MAX_RESULT: usize,
        const PROGRAM_BLOCK_SIZE: usize,
        const UI_BLOCK_SIZE: usize,
    > Iterator for ProgramLoader<'a, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>
{
    type Item = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_message()
    }
}

impl<
        'a,
        const MAX_ARGS: usize,
        const MAX_RESULT: usize,
        const PROGRAM_BLOCK_SIZE: usize,
        const UI_BLOCK_SIZE: usize,
    > ProgramLoader<'a, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>
{
    fn new(request_id: RequestId, program: &'a [ProgramWord], ui_state: &'a [u8]) -> Self {
        Self {
            request_id,
            next_program_block: 0,
            next_ui_block: 0,
            next_offset: 0,
            program,
            ui_state,
            ui_offset: 0,
            finished: false,
        }
    }

    // BUG: This should be changed to return Result
    //      Right now there are a bunch of errors
    //      which are turned in to None.
    pub fn next_message(
        &mut self,
    ) -> Option<Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>> {
        if self.program.len() > self.next_offset {
            let request_id = self.request_id;
            let start = self.next_offset;
            let end = start
                .checked_add(PROGRAM_BLOCK_SIZE)
                .map(|end| min(self.program.len(), end))?;
            let chunk = self.program.get(start..end)?;
            let next_offset = self.next_offset.checked_add(chunk.len())?;
            self.next_offset = next_offset;

            let Ok(block) = Vec::from_slice(chunk) else {
                return None; // This should not happen
            };

            let block_number = self.next_program_block;
            let next_block = self.next_program_block.checked_add(1)?;
            self.next_program_block = next_block;

            let message = if start == 0 {
                Protocol::LoadProgram {
                    request_id,
                    size: self.program.len() as u32,
                    ui_state_size: self.ui_state.len() as u32,
                    block_number,
                    block,
                }
            } else {
                Protocol::ProgramBlock {
                    request_id,
                    block_number,
                    block,
                }
            };

            return Some(message);
        }

        if self.ui_state.len() > self.ui_offset {
            let request_id = self.request_id;
            let start = self.ui_offset;
            let end = start
                .checked_add(UI_BLOCK_SIZE)
                .map(|end| min(self.ui_state.len(), end))?;
            let chunk = self.ui_state.get(start..end)?;
            let next_offset = self.ui_offset.checked_add(chunk.len())?;
            self.ui_offset = next_offset;

            let Ok(block) = Vec::from_slice(chunk) else {
                return None;
            };
            let block_number = self.next_ui_block;
            let next_block = self.next_ui_block.checked_add(1)?;
            self.next_ui_block = next_block;

            return Some(Protocol::UiStateBlock {
                request_id,
                total_size: self.ui_state.len() as u32,
                block_number,
                block,
            });
        }

        if self.finished {
            None
        } else {
            self.finished = true;
            Some(Protocol::FinishProgram {
                request_id: self.request_id,
            })
        }
    }
}
