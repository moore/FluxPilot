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
use light_machine::{MachineError, Program, Word};
use postcard::from_bytes_cobs;
use protocol::{Protocol, FunctionId, ErrorType};
use thiserror_no_std::Error;

use crate::protocol::{MessageType, RequestId};

#[derive(Error, Debug)]
pub enum StorageError {
    ProgramTooLarge,
    UnalignedWrite,
    UnknownProgram,
    InvalidProgram(MachineError),
    UnexpectedBlock,
}
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

    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError>;
    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError>;
    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError>;
    fn get_program<'a, 'b>(
        &'a mut self,
        program_number: ProgramNumber,
        globals: &'b mut [Word],
    ) -> Result<Program<'a, 'b>, StorageError>;
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
    S: Storage,
> {
    storage: &'a mut S,
    memory: &'b mut [Word],
    loader: Option<CurrentLoader<S>>,
}

impl<
    'a,
    'b,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    S: Storage,
> Pliot<'a, 'b, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, S>
{
    pub fn new<'c: 'a, 'd: 'b>(storage: &'a mut S, memory: &'b mut [Word]) -> Self {
        Self {
            storage,
            memory,
            loader: None,
        }
    }

    pub fn process_message<const STACK_SIZE: usize>(
        &mut self,
        stack: &mut Vec<Word, STACK_SIZE>,
        in_buff: &mut [u8],
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let message: Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE> = from_bytes_cobs(in_buff)?;

        let sent_len = match message {
            Protocol::Call {
                request_id,
                function,
                args,
            } => {
                let results = self.call(stack, function, &args)?;
                
                let result = Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>::Return {
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
                block_number,
                block,
            } => {
                let mut loader = self.storage.get_program_loader(size)?;
                match self.storage
                    .add_block(&mut loader, block_number, block.as_slice()) {
                        Ok(_) => {},
                        Err(error ) => match error {
                            StorageError::UnexpectedBlock => {
                                Self::write_unexpected_block(Some(request_id), block_number, out_buff)?;
                            }
                            StorageError::InvalidProgram(_error) => {
                                Self::write_error(Some(request_id), ErrorType::InvalidProgram, out_buff)?;
                            },
                            StorageError::ProgramTooLarge => {
                                Self::write_error(Some(request_id), ErrorType::ProgramTooLarge, out_buff)?;
                            },
                            StorageError::UnalignedWrite => {
                                Self::write_error(Some(request_id), ErrorType::UnalignedWrite, out_buff)?;
                            },
                            StorageError::UnknownProgram => {
                                Self::write_error(Some(request_id), ErrorType::UnknownProgram, out_buff)?;
                            }
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
                            0
                        }
                    }
                }
            }
        };

        Ok(sent_len)
    }


    pub fn call<const STACK_SIZE: usize>(&mut self, stack: &mut Vec<Word, STACK_SIZE>, function: FunctionId, args: &Vec<Word, MAX_ARGS>) -> Result<Vec<Word, MAX_RESULT>, PliotError> {
        let Ok(function_index) = function.function_index.try_into() else {
            return Err(PliotError::FunctionIndexOutOfRange);
        };

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

        let results: Vec<Word, MAX_RESULT> = stack.into_iter().map(|i| *i).collect();

       Ok(results)
    }

   pub fn get_led_color<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        index: u16,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(u8, u8, u8), PliotError> {
        let progroam_unmber = ProgramNumber(0);
        let mut program = self.storage.get_program(progroam_unmber, self.memory)?;
        let result = program.get_led_color(machine_number, index, stack)?;
        Ok(result)
    }

    fn write_unexpected_message_type(
        request_id: Option<RequestId>,
        message_type: MessageType,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let error_type = ErrorType::UnexpectedMessageType(message_type);
        Self::write_error(request_id, error_type, out_buff)
    }

    fn write_unexpected_block(
        request_id: Option<RequestId>,
        block_number: u32,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let error_type = ErrorType::UnexpectedProgramBlock(block_number);
        Self::write_error(request_id, error_type, out_buff)
    }

    fn write_error(
        request_id: Option<RequestId>,
        error_type: ErrorType,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let result = Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>::Error {
            request_id,
            error_type,
        };

        let wrote = postcard::to_slice_cobs(&result, out_buff)?;

        Ok(wrote.len())
    }
}




#[cfg(test)]
mod test;
