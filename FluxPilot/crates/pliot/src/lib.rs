#![no_std]

#![cfg_attr(
    not(test),
    deny(
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::indexing_slicing,
        clippy::string_slice,
        clippy::arithmetic_side_effects,
        clippy::panicking_unwrap,
        clippy::out_of_bounds_indexing,
        clippy::panic_in_result_fn,
        clippy::unwrap_in_result,
    )
)]
#![cfg_attr(not(test), warn(clippy::missing_panics_doc))]

pub mod meme_storage;
pub mod protocol;

use heapless::Vec;
use light_machine::{MachineError, Program, ProgramWord, StackWord};
use postcard::from_bytes_cobs;
use protocol::{ErrorLocation, Protocol, FunctionId, ErrorType};
use thiserror_no_std::Error;

use crate::protocol::{MessageType, RequestId};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StorageErrorKind {
    ProgramTooLarge,
    ProgramIncomplete,
    UnalignedWrite,
    WriteFailed,
    InvalidHeader,
    UnknownProgram,
    InvalidProgram,
    UnexpectedBlock,
    UiStateTooLarge,
    UiStateIncomplete,
    UiStateReadOutOfBounds,
}

#[derive(Error, Debug)]
pub enum StorageError {
    ProgramTooLarge { location: ErrorLocation },
    ProgramIncomplete { location: ErrorLocation },
    UnalignedWrite { location: ErrorLocation },
    WriteFailed { location: ErrorLocation },
    InvalidHeader { location: ErrorLocation },
    UnknownProgram { location: ErrorLocation },
    InvalidProgram { source: MachineError, location: ErrorLocation },
    UnexpectedBlock { location: ErrorLocation },
    UiStateTooLarge { location: ErrorLocation },
    UiStateIncomplete { location: ErrorLocation },
    UiStateReadOutOfBounds { location: ErrorLocation },
}

impl StorageError {
    #[track_caller]
    pub fn new(kind: StorageErrorKind) -> Self {
        let location = ErrorLocation::capture();
        match kind {
            StorageErrorKind::ProgramTooLarge => StorageError::ProgramTooLarge { location },
            StorageErrorKind::ProgramIncomplete => StorageError::ProgramIncomplete { location },
            StorageErrorKind::UnalignedWrite => StorageError::UnalignedWrite { location },
            StorageErrorKind::WriteFailed => StorageError::WriteFailed { location },
            StorageErrorKind::InvalidHeader => StorageError::InvalidHeader { location },
            StorageErrorKind::UnknownProgram => StorageError::UnknownProgram { location },
            StorageErrorKind::InvalidProgram => StorageError::InvalidProgram {
                source: MachineError::InvalidOp(0),
                location,
            },
            StorageErrorKind::UnexpectedBlock => StorageError::UnexpectedBlock { location },
            StorageErrorKind::UiStateTooLarge => StorageError::UiStateTooLarge { location },
            StorageErrorKind::UiStateIncomplete => StorageError::UiStateIncomplete { location },
            StorageErrorKind::UiStateReadOutOfBounds => {
                StorageError::UiStateReadOutOfBounds { location }
            }
        }
    }

    #[track_caller]
    pub fn invalid_program(source: MachineError) -> Self {
        StorageError::InvalidProgram {
            source,
            location: ErrorLocation::capture(),
        }
    }

    pub fn kind(&self) -> StorageErrorKind {
        match self {
            StorageError::ProgramTooLarge { .. } => StorageErrorKind::ProgramTooLarge,
            StorageError::ProgramIncomplete { .. } => StorageErrorKind::ProgramIncomplete,
            StorageError::UnalignedWrite { .. } => StorageErrorKind::UnalignedWrite,
            StorageError::WriteFailed { .. } => StorageErrorKind::WriteFailed,
            StorageError::InvalidHeader { .. } => StorageErrorKind::InvalidHeader,
            StorageError::UnknownProgram { .. } => StorageErrorKind::UnknownProgram,
            StorageError::InvalidProgram { .. } => StorageErrorKind::InvalidProgram,
            StorageError::UnexpectedBlock { .. } => StorageErrorKind::UnexpectedBlock,
            StorageError::UiStateTooLarge { .. } => StorageErrorKind::UiStateTooLarge,
            StorageError::UiStateIncomplete { .. } => StorageErrorKind::UiStateIncomplete,
            StorageError::UiStateReadOutOfBounds { .. } => StorageErrorKind::UiStateReadOutOfBounds,
        }
    }

    pub fn location(&self) -> &ErrorLocation {
        match self {
            StorageError::ProgramTooLarge { location }
            | StorageError::ProgramIncomplete { location }
            | StorageError::UnalignedWrite { location }
            | StorageError::WriteFailed { location }
            | StorageError::InvalidHeader { location }
            | StorageError::UnknownProgram { location }
            | StorageError::InvalidProgram { location, .. }
            | StorageError::UnexpectedBlock { location }
            | StorageError::UiStateTooLarge { location }
            | StorageError::UiStateIncomplete { location }
            | StorageError::UiStateReadOutOfBounds { location } => location,
        }
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ProgramNumber(pub(crate) usize);

impl ProgramNumber {
    pub fn new(number: usize) -> Self{
        ProgramNumber(number)
    }

    pub fn value(&self) -> usize {
        self.0
    }
}

pub trait Storage {
    type L: Sized;

    fn get_program_loader(
        &mut self,
        size: u32,
        ui_state_size: u32,
    ) -> Result<Self::L, StorageError>;
    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[ProgramWord],
    ) -> Result<(), StorageError>;
    fn add_ui_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[u8],
    ) -> Result<(), StorageError>;
    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError>;
    fn get_program<'a, 'b>(
        &'a mut self,
        program_number: ProgramNumber,
        globals: &'b mut [ProgramWord],
    ) -> Result<Program<'a, 'b>, StorageError>;
    fn get_ui_state_len(&mut self, program_number: ProgramNumber) -> Result<u32, StorageError>;
    fn read_ui_state_block(
        &mut self,
        program_number: ProgramNumber,
        offset: u32,
        out: &mut [u8],
    ) -> Result<usize, StorageError>;
}

#[derive(Error, Debug)]
pub enum PliotError {
    Postcard(#[from] postcard::Error),
    MachineError(#[from] light_machine::MachineError),
    FunctionIndexOutOfRange,
    OutBufToSmall,
    ResultTooLarge,
    StorageError(#[from] StorageError),
}

struct CurrentLoader<S: Storage> {
    loader: S::L,
    request_id: RequestId,
}
pub struct Pliot<
    'a,
    'b,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    S: Storage,
> {
    storage: &'a mut S,
    memory: &'b mut [ProgramWord],
    loader: Option<CurrentLoader<S>>,
}

impl<
    'a,
    'b,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    S: Storage,
> Pliot<'a, 'b, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, S>
{
    pub fn new<'c: 'a, 'd: 'b>(storage: &'a mut S, memory: &'b mut [ProgramWord]) -> Self {
        Self {
            storage,
            memory,
            loader: None,
        }
    }

    pub fn init<const STACK_SIZE: usize>(
        &mut self,
        stack: &mut Vec<StackWord, STACK_SIZE>,
    ) -> Result<(), PliotError> {
        let progroam_unmber = ProgramNumber(0);
        let mut program = self.storage.get_program(progroam_unmber, self.memory)?;
        let machine_count = program.machine_count()?;
        for machine_index in 0..machine_count {
            stack.clear();
            program.init_machine(machine_index, stack)?;
        }
        Ok(())
    }

    pub fn machine_count(&mut self) -> Result<ProgramWord, PliotError> {
        let progroam_unmber = ProgramNumber(0);
        let program = self.storage.get_program(progroam_unmber, self.memory)?;
        Ok(program.machine_count()?)
    }


    pub fn process_message<const STACK_SIZE: usize>(
        &mut self,
        stack: &mut Vec<StackWord, STACK_SIZE>,
        in_buff: &mut [u8],
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let message: Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
            from_bytes_cobs(in_buff)?;

        let sent_len = match message {
            Protocol::Call {
                request_id,
                function,
                args,
            } => {
                let results = self.call(stack, function, &args)?;
                
                let result =
                    Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>::Return {
                    request_id,
                    result: results,
                };

                let wrote = postcard::to_slice_cobs(&result, out_buff)?;

                wrote.len()
            }

            Protocol::Error { request_id, .. } => {
                // right now we send Error(s) we don't receive them but possibly
                // one day we'll want to make RCPs aginst the UI? For now return
                // a error if we get an error :P
                Self::write_unexpected_message_type(request_id, MessageType::Error, out_buff)?
            }

            Protocol::Return { request_id, .. } => {
                // right now we send Return(s) we don't receive them but possibly
                // one day we'll want to make RCPs aginst the UI? For now return
                // a error if we get an error :P
                Self::write_unexpected_message_type(Some(request_id), MessageType::Return,out_buff)?
            }

            Protocol::Notifacation { .. } => {
                // right now we send Notifacation(s) we don't receive them but possibly
                // one day we'll want to make RCPs aginst the UI? For now return
                // a error if we get an error :P
                Self::write_unexpected_message_type(None, MessageType::Notifacation, out_buff)?
            }

            Protocol::LoadProgram {
                request_id,
                size,
                ui_state_size,
                block_number,
                block,
            } => {
                let mut loader = self.storage.get_program_loader(size, ui_state_size)?;
                match self.storage
                    .add_block(&mut loader, block_number, block.as_slice()) {
                        Ok(_) => {},
                        Err(error) => {
                            let location = Some(error.location().clone());
                            let error_type = match error.kind() {
                                StorageErrorKind::UnexpectedBlock => {
                                    ErrorType::UnexpectedProgramBlock(block_number)
                                }
                                StorageErrorKind::InvalidProgram => ErrorType::InvalidProgram,
                                StorageErrorKind::ProgramTooLarge => ErrorType::ProgramTooLarge,
                                StorageErrorKind::ProgramIncomplete => ErrorType::ProgramIncomplete,
                                StorageErrorKind::UiStateTooLarge => ErrorType::UiStateTooLarge,
                                StorageErrorKind::UiStateIncomplete => ErrorType::UiStateIncomplete,
                                StorageErrorKind::UiStateReadOutOfBounds => {
                                    ErrorType::UiStateReadOutOfBounds
                                }
                                StorageErrorKind::UnalignedWrite => ErrorType::UnalignedWrite,
                                StorageErrorKind::WriteFailed => ErrorType::WriteFailed,
                                StorageErrorKind::InvalidHeader => ErrorType::InvalidHeader,
                                StorageErrorKind::UnknownProgram => ErrorType::UnknownProgram,
                            };
                            Self::write_error(
                                Some(request_id),
                                error_type,
                                location,
                                out_buff,
                            )?;
                        }
                }
                let current_loader = CurrentLoader { loader, request_id };
                self.loader = Some(current_loader);
                0
            }

            Protocol::ProgramBlock {
                request_id,
                block_number,
                block,
            } => {
                match &mut self.loader {
                    None => {
                        // BOOG: should return unexpted request it
                        Self::write_unexpected_message_type(Some(request_id), MessageType::ProgramBlock, out_buff)?
                    }
                    Some(current) => {
                        if current.request_id != request_id {
                            // BOOG: should return unexpted request it
                            Self::write_unexpected_message_type(Some(request_id), MessageType::ProgramBlock, out_buff)?
                        } else {
                            self.storage.add_block(
                                &mut current.loader,
                                block_number,
                                block.as_slice(),
                            )?;
                            0
                        }
                    }
                }
            }

            Protocol::UiStateBlock {
                request_id,
                block_number,
                block,
                ..
            } => {
                match &mut self.loader {
                    None => Self::write_unexpected_message_type(
                        Some(request_id),
                        MessageType::UiStateBlock,
                        out_buff,
                    )?,
                    Some(current) => {
                        if current.request_id != request_id {
                            Self::write_unexpected_message_type(
                                Some(request_id),
                                MessageType::UiStateBlock,
                                out_buff,
                            )?
                        } else {
                            self.storage.add_ui_block(
                                &mut current.loader,
                                block_number,
                                block.as_slice(),
                            )?;
                            0
                        }
                    }
                }
            }

            Protocol::ReadUiState {
                request_id,
                block_number,
            } => {
                let result: Result<usize, PliotError> = (|| {
                    let program_number = ProgramNumber(0);
                    let total_size = self.storage.get_ui_state_len(program_number)?;
                    if total_size == 0 {
                        let empty: Vec<u8, UI_BLOCK_SIZE> = Vec::new();
                        let response =
                            Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>::UiStateBlock {
                                request_id,
                                total_size,
                                block_number,
                                block: empty,
                            };
                        let wrote = postcard::to_slice_cobs(&response, out_buff)?;
                        Ok(wrote.len())
                    } else {
                        let offset = block_number
                            .checked_mul(UI_BLOCK_SIZE as u32)
                            .ok_or(StorageError::new(StorageErrorKind::UiStateReadOutOfBounds))?;
                        let mut temp = [0u8; UI_BLOCK_SIZE];
                        let read = self.storage.read_ui_state_block(
                            program_number,
                            offset,
                            temp.as_mut_slice(),
                        )?;
                        let mut block: Vec<u8, UI_BLOCK_SIZE> = Vec::new();
                        block
                            .extend_from_slice(&temp[..read])
                            .map_err(|_| StorageError::new(StorageErrorKind::UiStateTooLarge))?;
                        let response =
                            Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>::UiStateBlock {
                                request_id,
                                total_size,
                                block_number,
                                block,
                            };
                        let wrote = postcard::to_slice_cobs(&response, out_buff)?;
                        Ok(wrote.len())
                    }
                })();

                match result {
                    Ok(len) => len,
                    Err(error) => {
                        let (error_type, location) =
                            Self::error_type_for_read_ui_state(error, block_number);
                        match Self::write_error(
                            Some(request_id),
                            error_type,
                            location,
                            out_buff,
                        ) {
                            Ok(len) => len,
                            Err(_) => 0,
                        }
                    }
                }
            }

            Protocol::FinishProgram { request_id } => {
                let current = self.loader.take();
                match current {
                    None => {
                        // BOOG: should return unexpted request it
                        Self::write_unexpected_message_type(Some(request_id), MessageType::FinishProgram, out_buff)?
                    }
                    Some(current) => {
                        if current.request_id != request_id {
                            // BOOG: should return unexpted request it
                            self.loader = Some(current);
                            Self::write_unexpected_message_type(Some(request_id), MessageType::FinishProgram, out_buff)?
                        } else {
                            self.storage.finish_load(current.loader)?;
                            self.init(stack)?;
                            0
                        }
                    }
                }
            }
        };

        Ok(sent_len)
    }


    pub fn call<const STACK_SIZE: usize>(
        &mut self,
        stack: &mut Vec<StackWord, STACK_SIZE>,
        function: FunctionId,
        args: &Vec<StackWord, MAX_ARGS>,
    ) -> Result<Vec<StackWord, MAX_RESULT>, PliotError> {
        let Ok(function_index) = function.function_index.try_into() else {
            return Err(PliotError::FunctionIndexOutOfRange);
        };
        stack.clear();

        for arg in args {
            if stack.push(*arg).is_err() {
                Err(MachineError::StackOverflow)?;
            }
        }
        
        let progroam_unmber = ProgramNumber(0);
        let mut program = self.storage.get_program(progroam_unmber, self.memory)?;

        program.call(function.machine_index, function_index, stack)?;

        if stack.len() > MAX_RESULT {
            return Err(PliotError::ResultTooLarge);
        }

        let results: Vec<StackWord, MAX_RESULT> = stack.into_iter().map(|i| *i).collect();

       Ok(results)
    }

   pub fn get_led_color<const STACK_SIZE: usize>(
        &mut self,
        machine_number: ProgramWord,
        index: u16,
        tick: u16,
        stack: &mut Vec<StackWord, STACK_SIZE>,
    ) -> Result<(u8, u8, u8), PliotError> {
        let progroam_unmber = ProgramNumber(0);
        let mut program = self.storage.get_program(progroam_unmber, self.memory)?;
        let result = program.get_led_color(machine_number, index, tick, stack)?;
        Ok(result)
    }

    fn write_unexpected_message_type(
        request_id: Option<RequestId>,
        message_type: MessageType,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let error_type = ErrorType::UnexpectedMessageType(message_type);
        Self::write_error(request_id, error_type, None, out_buff)
    }

    fn write_error(
        request_id: Option<RequestId>,
        error_type: ErrorType,
        location: Option<ErrorLocation>,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let result = Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>::Error {
            request_id,
            error_type,
            location,
        };

        let wrote = postcard::to_slice_cobs(&result, out_buff)?;

        Ok(wrote.len())
    }

    fn error_type_for_read_ui_state(
        error: PliotError,
        block_number: u32,
    ) -> (ErrorType, Option<ErrorLocation>) {
        match error {
            PliotError::StorageError(storage) => {
                let location = Some(storage.location().clone());
                let error_type = match storage.kind() {
                    StorageErrorKind::ProgramTooLarge => ErrorType::ProgramTooLarge,
                    StorageErrorKind::ProgramIncomplete => ErrorType::ProgramIncomplete,
                    StorageErrorKind::UnalignedWrite => ErrorType::UnalignedWrite,
                    StorageErrorKind::WriteFailed => ErrorType::WriteFailed,
                    StorageErrorKind::InvalidHeader => ErrorType::InvalidHeader,
                    StorageErrorKind::UnknownProgram => ErrorType::UnknownProgram,
                    StorageErrorKind::InvalidProgram => ErrorType::InvalidProgram,
                    StorageErrorKind::UnexpectedBlock => {
                        ErrorType::UnexpectedProgramBlock(block_number)
                    }
                    StorageErrorKind::UiStateTooLarge => ErrorType::UiStateTooLarge,
                    StorageErrorKind::UiStateIncomplete => ErrorType::UiStateIncomplete,
                    StorageErrorKind::UiStateReadOutOfBounds => ErrorType::UiStateReadOutOfBounds,
                };
                (error_type, location)
            }
            PliotError::Postcard(_) => (ErrorType::InvalidMessage, None),
            PliotError::MachineError(_) => (ErrorType::InvalidProgram, None),
            PliotError::FunctionIndexOutOfRange => (ErrorType::InvalidMessage, None),
            PliotError::OutBufToSmall => (ErrorType::InvalidMessage, None),
            PliotError::ResultTooLarge => (ErrorType::InvalidMessage, None),
        }
    }
}




#[cfg(test)]
mod test;
