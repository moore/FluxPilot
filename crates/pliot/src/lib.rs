#![no_std]
pub mod meme_storage;
pub mod protocol;

use heapless::Vec;
use light_machine::{MachineError, Program, Word};
use postcard::from_bytes_cobs;
use protocol::{Protocol, FunctionId};
use thiserror_no_std::Error;

use crate::protocol::RequestId;

#[derive(Error, Debug)]
pub enum StorageError {
    ProgramTooLarge,
    UnknownProgram,
    InvalidProgram(MachineError),
    UnexpectedBlock,
}
pub struct ProgramNumber(pub(crate) usize);

pub trait Storage {
    type L: Sized;

    fn get_program_loader<'a>(&'a mut self, size: u32) -> Result<Self::L, StorageError>;
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
                Self::wrire_unexpected_message_type(request_id, out_buff)?
            }

            Protocol::Return { request_id, .. } => {
                // right now we send Return(s) we don't receive them but possibly
                // one day we'll want to make RCPs aginst the UI? For now return
                // a error if we get an error :P
                Self::wrire_unexpected_message_type(Some(request_id), out_buff)?
            }

            Protocol::Notifacation { .. } => {
                // right now we send Notifacation(s) we don't receive them but possibly
                // one day we'll want to make RCPs aginst the UI? For now return
                // a error if we get an error :P
                Self::wrire_unexpected_message_type(None, out_buff)?
            }

            Protocol::LoadProgram {
                request_id,
                size,
                block_number,
                block,
            } => {
                let mut loader = self.storage.get_program_loader(size)?;
                self.storage
                    .add_block(&mut loader, block_number, block.as_slice())?;
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
                        Self::wrire_unexpected_message_type(None, out_buff)?
                    }
                    Some(current) => {
                        if current.request_id != request_id {
                            // BOOG: should return unexpted request it
                            Self::wrire_unexpected_message_type(None, out_buff)?
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
                        Self::wrire_unexpected_message_type(None, out_buff)?
                    }
                    Some(current) => {
                        if current.request_id != request_id {
                            // BOOG: should return unexpted request it
                            self.loader = Some(current);
                            Self::wrire_unexpected_message_type(None, out_buff)?
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

    fn wrire_unexpected_message_type(
        request_id: Option<RequestId>,
        out_buff: &mut [u8],
    ) -> Result<usize, PliotError> {
        let result = Protocol::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>::Error {
            request_id,
            error_type: protocol::ErrorType::UnexpectedMessageType,
        };

        let wrote = postcard::to_slice_cobs(&result, out_buff)?;

        Ok(wrote.len())
    }
}

#[cfg(test)]
mod test;
