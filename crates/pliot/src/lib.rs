pub mod protocol;

use heapless::Vec;
use light_machine::{MachineError, Program, Word};
use postcard::from_bytes_cobs;
use protocol::Protocol;
use thiserror_no_std::Error;

use crate::protocol::RequestId;

#[derive(Error, Debug)]
pub enum PliotError {
    Postcard(#[from] postcard::Error),
    MachineError(#[from] light_machine::MachineError),
    FunctionIndexOutOfRange,
    OutBufToSmall,
    ResultTooLarge,
}
pub struct Pliot<
    'a,
    'b,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
> {
    program: Program<'a, 'b>,
}

impl<'a, 'b, const MAX_ARGS: usize, const MAX_RESULT: usize, const PROGRAM_BLOCK_SIZE: usize>
    Pliot<'a, 'b, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>
{
    pub fn new<'c: 'a, 'd: 'b>(program: Program<'c, 'd>) -> Self {
        Self { program }
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
                println!("Protocol::Call");
                let Ok(function_index) = function.funtion_index.try_into() else {
                    return Err(PliotError::FunctionIndexOutOfRange);
                };

                for arg in args {
                    if stack.push(arg).is_err() {
                        Err(MachineError::StackOverflow)?;
                    }
                }
                self.program
                    .call(function.machine_index, function_index, stack)?;

                if stack.len() > MAX_RESULT {
                    return Err(PliotError::ResultTooLarge);
                }

                let results: Vec<u16, MAX_RESULT> = stack.into_iter().map(|i| *i).collect();

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
            } => 0,

            Protocol::ProgramBlock {
                request_id,
                block_number,
                block,
            } => 0,

            Protocol::FinishProgram { request_id } => 0,
        };

        Ok(sent_len)
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
