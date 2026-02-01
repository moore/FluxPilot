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

use core::mem::transmute;
use heapless::Vec;
use thiserror_no_std::Error;
use variant_count::VariantCount;

use crate::builder::FunctionIndex;

pub mod builder;
pub mod assembler;

#[cfg(test)]
mod assembler_test;
/// This module implments the vitural machine for FluxPilot.
/// A machine takes three memory regions when it is initilized:
/// `
///     static_data: &'a [Word],
///     globals: &'b mut [Word],
///     program: &'c [Word],
/// `
/// The `static_data` is used to hold any constancs that the machine
/// program needs. The `globals`` holds any mutibal state that needs
/// to persist betweeen calls in to the machine. The `program` holds
/// instructions and valued used in the evalution of the machine program.
///
/// The `static_data`` and `program` may hold data assocated with more
/// that one machine to fisilitate sharing of data and opperations between
/// machines.
///
/// During initilzation additional pointers to entrypoints in the program
/// are provieded, ex a offset in to the program to the function which
/// initlises the `globals` for the machine as well as a entry point for
/// the main function.
///
/// It is a stack based vitural machine where all opps and
/// values are u16. This allow the mixture of instructions
/// and program data in the program section and allows the
/// stack to have a single type.
pub type Word = u16;

fn get_mut_or<E>(slice: &mut [Word], index: usize, err: E) -> Result<&mut Word, E> {
    slice.get_mut(index).ok_or(err)
}

fn set_value<E>(slice: &mut [Word], index: usize, value: Word, err: E) -> Result<(), E> {
    *get_mut_or(slice, index, err)? = value;
    Ok(())
}

#[repr(u16)] // Must match Word
#[derive(VariantCount, Debug)]
pub enum Ops {
    Pop,
    Push,
    BranchLessThan,
    BranchLessThanEq,
    BranchGreaterThan,
    BranchGreaterThanEq,
    BranchEqual,
    And,
    Or,
    Xor,
    Not,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseNot,
    Multiply,
    Divide,
    Mod,
    Add,
    Subtract,
    Load,
    Store,
    LoadStatic,
    Jump,
    Exit,
    Call,
    StackLoad,
    StackStore,
    Dup,
    Swap,
    Return,
}

impl From<Ops> for Word {
    fn from(op: Ops) -> Word {
        op as Word
    }
}

impl TryFrom<Word> for Ops {
    type Error = MachineError;
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        // SAFTY: we require Ops to be in range of u16
        // with `repr` macro.
        if value >= Ops::VARIANT_COUNT as u16 {
            return Err(MachineError::InvalidOp(value));
        }

        // SAFTY: We just check that the value is in range.
        let op = unsafe { transmute::<Word, Self>(value) };
        Ok(op)
    }
}

#[derive(Error, Debug)]
pub enum MachineError {
    //#[error("the value {0} is out of the program bounds")]
    //InstructionPointerOutOfBounds(usize),
    #[error("the value {0} is out of the globals bounds")]
    OutOfBoundsGlobalsAccess(usize),
    #[error("the value {0} is an invalid opcode")]
    InvalidOp(Word),
    #[error("the pop op code was executed on an empty stack")]
    PopOnEmptyStack,
    #[error("attempted opperation would overflow the stack")]
    StackOverflow,
    #[error("attempted opperation would underflow the stack")]
    StackUnderFlow,
    #[error("there are not enogh arguments to call the function")]
    TwoFewArguments,
    #[error("word {0} out of range of color value")]
    ColorOutOfRange(Word),
    #[error("indesx {0} out of range of static data")]
    OutOfBoudsStaticRead(usize),
    #[error("index {0} out of range of static data")]
    GlobalsBufferTooSmall(Word),
    #[error("index {0} out of range for machine index")]
    MachineIndexOutOfRange(Word),
}

pub const MACHINE_COUNT_OFFSET: usize = 0;
pub const GLOBALS_SIZE_OFFSET: usize = MACHINE_COUNT_OFFSET + 1;
pub const MACHINE_TABLE_OFFSET: usize = GLOBALS_SIZE_OFFSET + 1;
pub const MACHINE_FUNCTIONS_OFFSET: usize = 2;

const INIT_OFFSET: usize = 0;
const GET_COLOR_OFFSET: usize = INIT_OFFSET + 1;

#[derive(Debug)]
pub struct MachineDescriptor<const FUNCTION_COUNT_MAX: usize> {
    pub functions: Vec<FunctionIndex, FUNCTION_COUNT_MAX>,
}

impl<const FUNCTION_COUNT_MAX: usize> MachineDescriptor<FUNCTION_COUNT_MAX> {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    pub fn add_function(&mut self, index: FunctionIndex) -> Result<(), FunctionIndex> {
        self.functions.push(index)
    }
}

impl<const FUNCTION_COUNT_MAX: usize> Default for MachineDescriptor<FUNCTION_COUNT_MAX> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ProgramDescriptor<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    pub length: usize,
    pub machines: Vec<MachineDescriptor<FUNCTION_COUNT_MAX>, MACHINE_COUNT_MAX>,
}

impl<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new() -> Self {
        Self {
            length: 0,
            machines: Vec::new(),
        }
    }

    pub fn add_machine(
        &mut self,
        machine_descriptor: MachineDescriptor<FUNCTION_COUNT_MAX>,
    ) -> Result<(), MachineDescriptor<FUNCTION_COUNT_MAX>> {
        self.machines.push(machine_descriptor)
    }
}

impl<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> Default
    for ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    fn default() -> Self {
        Self::new()
    }
}

pub struct Program<'a, 'b> {
    static_data: &'a [Word],
    globals: &'b mut [Word],
    frame_pointer: Word,
}

impl<'a, 'b> Program<'a, 'b> {
    pub fn new(static_data: &'a [Word], globals: &'b mut [Word]) -> Result<Self, MachineError> {
        let Some(globals_size) = static_data.get(GLOBALS_SIZE_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(GLOBALS_SIZE_OFFSET));
        };

        if *globals_size as usize > globals.len() {
            return Err(MachineError::GlobalsBufferTooSmall(*globals_size));
        }

        Ok(Self {
            static_data,
            globals,
            frame_pointer: 0,
        })
    }


    pub fn machine_count(&self) -> Result<Word, MachineError> {
        let Some(count) = self.static_data.get(MACHINE_COUNT_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(MACHINE_COUNT_OFFSET));
        };

        Ok(*count)
    }

    fn get_function_entry(
        &self,
        machine_number: Word,
        function_number: usize,
    ) -> Result<usize, MachineError> {
        if machine_number > self.machine_count()? {
            return Err(MachineError::MachineIndexOutOfRange(machine_number));
        };
        // BOOG check for function out of range.
        let Some(machine_slot) = (machine_number as usize).checked_add(MACHINE_TABLE_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(machine_number as usize));
        };
        let machine_index = read_static(machine_slot, self.static_data)?;
        let Some(index_function_index) = (machine_index as usize)
            .checked_add(MACHINE_FUNCTIONS_OFFSET)
            .and_then(|base| base.checked_add(function_number))
        else {
            return Err(MachineError::OutOfBoudsStaticRead(machine_index as usize));
        };
        let entry_point = read_static(index_function_index, self.static_data)?;
        Ok(entry_point as usize)
    }

    pub fn init_machine<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(), MachineError> {
        let entry_point = self.get_function_entry(machine_number, INIT_OFFSET)?;
        self.run(machine_number, entry_point, stack)?;
        Ok(())
    }

    pub fn get_led_color<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        index: u16,
        tick: u16,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(u8, u8, u8), MachineError> {
        if stack.len() < 3 {
            return Err(MachineError::TwoFewArguments);
        }
        stack.push(index).map_err(|_| MachineError::StackOverflow)?;
        stack.push(tick).map_err(|_| MachineError::StackOverflow)?;


        let entry_point = self.get_function_entry(machine_number, GET_COLOR_OFFSET)?;

        self.run(machine_number, entry_point, stack)?;

        let Some(blue) = stack.pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(green) = stack.pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(red) = stack.pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let red = word_to_color(red)?;
        let green = word_to_color(green)?;
        let blue = word_to_color(blue)?;

        Ok((red, green, blue))
    }

    pub fn call<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        function_number: usize,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(), MachineError> {
        let entry_point = self.get_function_entry(machine_number, function_number)?;

        self.run(machine_number, entry_point, stack)?;
        Ok(())
    }

    fn run<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        entry_point: usize,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(), MachineError> {
        let mut pc = entry_point;

        loop {
            let word = read_static(pc, self.static_data)?;
            let op = word.try_into()?;
            match op {
                Ops::Pop => {
                    if stack.pop().is_none() {
                        return Err(MachineError::PopOnEmptyStack);
                    }
                }
                Ops::Push => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;
                    push(stack, word)?;
                }
                Ops::BranchLessThan => {
                    let target = pop(stack)? as usize;
                    let (lhs, rhs) = pop2(stack)?;
                    if lhs < rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchLessThanEq => {
                    let target = pop(stack)? as usize;
                    let (lhs, rhs) = pop2(stack)?;
                    if lhs <= rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchGreaterThan => {
                    let target = pop(stack)? as usize;
                    let (lhs, rhs) = pop2(stack)?;
                    if lhs > rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchGreaterThanEq => {
                    let target = pop(stack)? as usize;
                    let (lhs, rhs) = pop2(stack)?;
                    if lhs >= rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchEqual => {
                    let target = pop(stack)? as usize;
                    let (lhs, rhs) = pop2(stack)?;
                    if lhs == rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::And => {
                    let (lhs, rhs) = pop2(stack)?;
                    let result = if lhs != 0 && rhs != 0 { 1 } else { 0 };
                    push(stack, result)?;
                }
                Ops::Or => {
                    let (lhs, rhs) = pop2(stack)?;
                    let result = if lhs != 0 || rhs != 0 { 1 } else { 0 };
                    push(stack, result)?;
                }
                Ops::Xor => {
                    let (lhs, rhs) = pop2(stack)?;
                    let result = if (lhs != 0) ^ (rhs != 0) { 1 } else { 0 };
                    push(stack, result)?;
                }
                Ops::Not => {
                    let value = pop(stack)?;
                    let result = if value == 0 { 1 } else { 0 };
                    push(stack, result)?;
                }
                Ops::BitwiseAnd => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs & rhs)?;
                }
                Ops::BitwiseOr => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs | rhs)?;
                }
                Ops::BitwiseXor => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs ^ rhs)?;
                }
                Ops::BitwiseNot => {
                    let value = pop(stack)?;
                    push(stack, !value)?;
                }
                Ops::Multiply => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs.wrapping_mul(rhs))?;
                }
                Ops::Divide => {
                    let (lhs, rhs) = pop2(stack)?;
                    let result = lhs
                        .checked_div(rhs)
                        .ok_or(MachineError::InvalidOp(word))?;
                    push(stack, result)?;
                }
                Ops::Mod => {
                    let (lhs, rhs) = pop2(stack)?;
                    let result = lhs
                        .checked_rem(rhs)
                        .ok_or(MachineError::InvalidOp(word))?;
                    push(stack, result)?;
                }
                Ops::Add => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs.wrapping_add(rhs))?;
                }
                Ops::Subtract => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, lhs.wrapping_sub(rhs))?;
                }
                Ops::Load => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<Word>() <= size_of::<usize>());
                    }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    let word = read_global(index, self.globals)?;

                    push(stack, word)?;
                }
                Ops::Store => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<Word>() <= size_of::<usize>());
                    }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    let word = pop(stack)?;

                    set_value(
                        self.globals,
                        index,
                        word,
                        MachineError::OutOfBoundsGlobalsAccess(index),
                    )?;
                }
                Ops::LoadStatic => {
                    let addr = pop(stack)? as usize;
                    let value = read_static(addr, self.static_data)?;
                    push(stack, value)?;
                }
                Ops::Jump => {
                    pc = pop(stack)? as usize;
                    continue;
                }
                Ops::StackLoad => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;
                    let index = self
                        .frame_pointer
                        .checked_add(offset)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let value = *stack
                        .get(index as usize)
                        .ok_or(MachineError::StackUnderFlow)?;
                    push(stack, value)?;
                }
                Ops::StackStore => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;
                    let index = self
                        .frame_pointer
                        .checked_add(offset)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let value = *stack.last().ok_or(MachineError::StackUnderFlow)?;
                    let slot = stack
                        .get_mut(index as usize)
                        .ok_or(MachineError::StackUnderFlow)?;
                    *slot = value;
                    let _ = pop(stack)?;
                }
                Ops::Dup => {
                    let value = *stack.last().ok_or(MachineError::StackUnderFlow)?;
                    push(stack, value)?;
                }
                Ops::Swap => {
                    let (lhs, rhs) = pop2(stack)?;
                    push(stack, rhs)?;
                    push(stack, lhs)?;
                }
                Ops::Exit => {
                    self.frame_pointer = 0;
                    break
                }
                Ops::Call => {
                    // Stack convention: ... args, arg_count, func_index
                    let function_index = pop(stack)? as usize;
                    let arg_count = pop(stack)? as usize;
                    let arg_start = stack
                        .len()
                        .checked_sub(arg_count)
                        .ok_or(MachineError::StackUnderFlow)?;
                    // Save current frame pointer so the callee can access its caller frame.
                    let saved_frame_pointer = self.frame_pointer;
                    // Precompute return PC so it can be pushed ahead of the callee's args.
                    let return_pc = next_pc(pc)? as Word;
                    // Insert return PC before the first argument for this call frame layout:
                    // [return_pc, saved_fp, arg0, arg1, ...]
                    stack
                        .insert(arg_start, return_pc)
                        .map_err(|_| MachineError::StackOverflow)?;
                    // Insert saved FP immediately after return PC.
                    let saved_pointer_index = arg_start
                        .checked_add(1)
                        .ok_or(MachineError::StackOverflow)?;
                    stack
                        .insert(saved_pointer_index, saved_frame_pointer)
                        .map_err(|_| MachineError::StackOverflow)?;
                    // Frame pointer points at arg0, which is now shifted by two slots.
                    let new_frame_pointer = arg_start
                        .checked_add(2)
                        .ok_or(MachineError::StackOverflow)?;
                    // Convert usize->Word safely; Word limits keep stack indexing bounded.
                    let new_frame_pointer =
                        Word::try_from(new_frame_pointer).map_err(|_| MachineError::StackOverflow)?;
                    self.frame_pointer = new_frame_pointer;
                    let entry_point = self.get_function_entry(machine_number, function_index)?;
                    self.run(machine_number, entry_point, stack)?;
                    // Restore caller's frame pointer after returning.
                    self.frame_pointer = saved_frame_pointer;
                    pc = return_pc as usize;
                    continue;
                }
                Ops::Return => {
                    // Read the return count operand that follows RET.
                    pc = next_pc(pc)?;
                    let return_count = read_static(pc, self.static_data)? as usize;
                    // Compute frame metadata positions relative to the current frame pointer.
                    let fp_index = usize::from(self.frame_pointer);
                    let return_pc_index = fp_index
                        .checked_sub(2)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let saved_fp_index = fp_index
                        .checked_sub(1)
                        .ok_or(MachineError::StackUnderFlow)?;
                    // Fetch return PC and the caller's frame pointer from the frame header.
                    let return_pc = *stack
                        .get(return_pc_index)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let saved_frame_pointer = *stack
                        .get(saved_fp_index)
                        .ok_or(MachineError::StackUnderFlow)?;
                    // Copy return values from the top of the stack before unwinding the frame.
                    let original_len = stack.len();
                    let return_values_start = original_len
                        .checked_sub(return_count)
                        .ok_or(MachineError::StackUnderFlow)?;
                    
                    for offset in 0..return_count {
                        let src_index = return_values_start
                            .checked_add(offset)
                            .ok_or(MachineError::StackUnderFlow)?;
                        let dest_index = return_pc_index
                            .checked_add(offset)
                            .ok_or(MachineError::StackOverflow)?;
                        let value = *stack
                            .get(src_index)
                            .ok_or(MachineError::StackUnderFlow)?;
                        let slot = stack
                            .get_mut(dest_index)
                            .ok_or(MachineError::StackUnderFlow)?;
                        *slot = value;
                    }

                    // Drop the call frame header and locals, keeping only the return values.
                    let new_len = return_pc_index
                            .checked_add(return_count)
                            .ok_or(MachineError::StackUnderFlow)?;
                    stack.truncate(new_len);
                    // Restore caller frame pointer and jump to saved return PC.
                    self.frame_pointer = saved_frame_pointer;
                    pc = return_pc as usize;
                    continue;
                }
            }
            pc = next_pc(pc)?;
        }

        Ok(())
    }
}

fn next_pc(pc: usize) -> Result<usize, MachineError> {
    pc.checked_add(1)
        .ok_or(MachineError::OutOfBoudsStaticRead(pc)) // BUG: This should be OverFlowError
}

fn read_static(index: usize, program: &[Word]) -> Result<Word, MachineError> {
    match program.get(index) {
        None => Err(MachineError::OutOfBoudsStaticRead(index)),
        Some(word) => Ok(*word),
    }
}

fn read_global(index: usize, globals: &[Word]) -> Result<Word, MachineError> {
    match globals.get(index) {
        None => Err(MachineError::OutOfBoundsGlobalsAccess(index)),
        Some(word) => Ok(*word),
    }
}

fn pop<const STACK_SIZE: usize>(
    stack: &mut Vec<Word, STACK_SIZE>,
) -> Result<Word, MachineError> {
    stack.pop().ok_or(MachineError::StackUnderFlow)
}

fn pop2<const STACK_SIZE: usize>(
    stack: &mut Vec<Word, STACK_SIZE>,
) -> Result<(Word, Word), MachineError> {
    let rhs = pop(stack)?;
    let lhs = pop(stack)?;
    Ok((lhs, rhs))
}

fn push<const STACK_SIZE: usize>(
    stack: &mut Vec<Word, STACK_SIZE>,
    value: Word,
) -> Result<(), MachineError> {
    if stack.push(value).is_err() {
        return Err(MachineError::StackOverflow);
    }
    Ok(())
}

fn word_to_color(word: Word) -> Result<u8, MachineError> {
    match word.try_into() {
        Ok(c) => Ok(c),
        Err(_) => Err(MachineError::ColorOutOfRange(word)),
    }
}

#[cfg(test)]
mod test;
