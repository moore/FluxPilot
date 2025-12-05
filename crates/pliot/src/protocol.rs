use heapless::Vec;
use light_machine;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(u64);

impl RequestId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

#[derive(Serialize, Deserialize)]
pub struct MachineId(u32);

#[derive(Serialize, Deserialize, Debug)]
pub struct FunctionId {
    pub machine_index: light_machine::Word,
    pub funtion_index: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ErrorType {
    UnknownResuestId(RequestId),
    UnexpectedProgramBlock(u32),
    UnknownMachine(u32),
    UnknownFucntion(u32),
    UnexpectedMessageType,

}

#[derive(Serialize, Deserialize, Debug)]
pub enum Protocol<const MAX_ARGS: usize, const MAX_RESULT: usize, const PROGRAM_BLOCK_SIZE: usize> {
    /// Call a function on a machine
    Call {
        request_id: RequestId,
        function: FunctionId,
        args: Vec<u16, MAX_ARGS>,
    },
    /// The return value of a call to a function
    Return {
        request_id: RequestId,
        result: Vec<u16, MAX_RESULT>,
    },
    /// Notification that a function was called on a machine
    Notifacation {
        function: FunctionId,
        result: Vec<u16, MAX_RESULT>,
    },
    /// Function call produced error.
    Error {
        request_id: Option<RequestId>,
        error_type: ErrorType,
    },
    /// Start new program load
    LoadProgram {
        request_id: RequestId,
        size: u32,
        block_number: u32,
        block: Vec<u8, PROGRAM_BLOCK_SIZE>,
    },
    /// Program Block
    ProgramBlock {
        request_id: RequestId,
        block_number: u32,
        block: Vec<u8, PROGRAM_BLOCK_SIZE>,
    },
    /// Finish the new program load
    FinishProgram { request_id: RequestId },
}

impl<const MAX_ARGS: usize, const MAX_RESULT: usize, const PROGRAM_BLOCK_SIZE: usize>
    Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>
{
    pub fn get_request_id(&self) -> Option<RequestId> {
        match self {
            Protocol::Call { request_id, .. } => Some(*request_id),
            Protocol::Return { request_id, .. } => Some(*request_id),
            Protocol::Notifacation { .. } => None,
            Protocol::Error { request_id, .. } => *request_id,
            Protocol::LoadProgram { request_id, .. } => Some(*request_id),
            Protocol::ProgramBlock { request_id, .. } => Some(*request_id),
            Protocol::FinishProgram { request_id, .. } => Some(*request_id),
        }
    }
}

pub struct Controler<
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
> {
    next_request: u64,
}

impl<const MAX_ARGS: usize, const MAX_RESULT: usize, const PROGRAM_BLOCK_SIZE: usize>
    Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>
{
    pub fn new() -> Self {
        Controler { next_request: 0 }
    }

    pub fn call(
        &mut self,
        function: FunctionId,
        args: Vec<u16, MAX_ARGS>,
    ) -> Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE> {
        let request_id = self.get_request_id();
        Protocol::Call {
            request_id,
            function,
            args,
        }
    }

    fn get_request_id(&mut self) -> RequestId {
        self.next_request += 1;
        RequestId(self.next_request)
    }
}
