#![no_std]

use core::mem::transmute;
use heapless::Vec;
use thiserror_no_std::Error;
use variant_count::VariantCount;

pub mod builder;
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
    Devide,
    Add,
    Subtract,
    Load,
    Store,
    LoadStatic,
    Jump,
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

pub struct Program<'a, 'b> {
    static_data: &'a [Word],
    globals: &'b mut [Word],
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
        let machine_slot = machine_number as usize + MACHINE_TABLE_OFFSET;
        let machine_index = read_static(machine_slot, self.static_data)?;
        let index_function_index =
            machine_index as usize + MACHINE_FUNCTIONS_OFFSET + function_number;
        let entry_point = read_static(index_function_index, self.static_data)?;
        Ok(entry_point as usize)
    }

    pub fn init_machine<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(), MachineError> {
        let entry_point = self.get_function_entry(machine_number, INIT_OFFSET)?;
        self.run(entry_point, stack)?;
        Ok(())
    }

    pub fn get_led_color<const STACK_SIZE: usize>(
        &mut self,
        machine_number: Word,
        index: u16,
        stack: &mut Vec<Word, STACK_SIZE>,
    ) -> Result<(u8, u8, u8), MachineError> {
        stack.push(index).map_err(|_| MachineError::StackOverflow)?;

        let entry_point = self.get_function_entry(machine_number, GET_COLOR_OFFSET)?;

        self.run(entry_point, stack)?;

        let Some(red) = stack.pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(green) = stack.pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(blue) = stack.pop() else {
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
        self.run(entry_point, stack)?;
        Ok(())
    }

    fn run<const STACK_SIZE: usize>(
        &mut self,
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
                    pc += 1;
                    let word = read_static(pc, self.static_data)?;
                    if let Err(_) = stack.push(word) {
                        return Err(MachineError::StackOverflow);
                    }
                }
                Ops::BranchLessThan => (),
                Ops::BranchLessThanEq => (),
                Ops::BranchGreaterThan => (),
                Ops::BranchGreaterThanEq => (),
                Ops::BranchEqual => (),
                Ops::And => (),
                Ops::Or => (),
                Ops::Xor => (),
                Ops::Not => (),
                Ops::BitwiseAnd => (),
                Ops::BitwiseOr => (),
                Ops::BitwiseXor => (),
                Ops::BitwiseNot => (),
                Ops::Multiply => (),
                Ops::Devide => (),
                Ops::Add => (),
                Ops::Subtract => (),
                Ops::Load => {
                    pc += 1;
                    let word = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<Word>() <= size_of::<usize>());
                    }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    let word = read_global(index, self.globals)?;

                    if let Err(_) = stack.push(word) {
                        return Err(MachineError::StackOverflow);
                    }
                }
                Ops::Store => {
                    pc += 1;
                    let word = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<Word>() <= size_of::<usize>());
                    }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    if index >= self.globals.len() {
                        return Err(MachineError::OutOfBoundsGlobalsAccess(index));
                    }

                    let Some(word) = stack.pop() else {
                        return Err(MachineError::StackUnderFlow);
                    };

                    self.globals[index] = word;
                }
                Ops::LoadStatic => (),
                Ops::Jump => (),
                Ops::Return => break,
            }
            pc += 1;
        }

        Ok(())
    }
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

fn word_to_color(word: Word) -> Result<u8, MachineError> {
    match word.try_into() {
        Ok(c) => Ok(c),
        Err(_) => return Err(MachineError::ColorOutOfRange(word)),
    }
}

#[cfg(test)]
mod test;
