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

use core::mem::{align_of, transmute, size_of};
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
///     static_data: &'a [ProgramWord],
///     globals: &'b mut [ProgramWord],
///     program: &'c [ProgramWord],
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
/// It is a stack based vitural machine where all program words
/// are u16 but stack values are u32. This allow the mixture of
/// instructions and program data in the program section while
/// expanding the runtime arithmetic domain.
pub type ProgramWord = u16;
pub type StackWord = u32;

struct ProgramMemory<'a> {
    globals: &'a mut [ProgramWord],
    stack: StackSlice<'a>,
}

impl<'a> ProgramMemory<'a> {
    fn split(
        memory: &'a mut [ProgramWord],
        globals_size: ProgramWord,
    ) -> Result<Self, MachineError> {
        let stack_bottom = stack_bottom_for_globals(globals_size);
        let memory_len = memory.len();
        if memory_len < stack_bottom {
            return Err(MachineError::MemoryBufferTooSmall {
                needed: stack_bottom,
                provided: memory_len,
            });
        }
        let (globals, stack_words) = memory.split_at_mut(stack_bottom);
        let stack = StackSlice::from_program_words(stack_words)?;
        Ok(Self { globals, stack })
    }
}

pub struct StackSlice<'a> {
    data: &'a mut [StackWord],
    len: usize,
}

impl<'a> StackSlice<'a> {
    fn from_program_words(words: &'a mut [ProgramWord]) -> Result<Self, MachineError> {
        const {
            assert!(size_of::<StackWord>().is_multiple_of(size_of::<ProgramWord>()));
        }
        let stack_ratio = size_of::<StackWord>()
            .checked_div(size_of::<ProgramWord>())
            .ok_or(MachineError::StackMemoryMisaligned)?;
        let ptr = words.as_mut_ptr();
        let ptr_addr = ptr as usize;
        if !ptr_addr.is_multiple_of(align_of::<StackWord>()) {
            return Err(MachineError::StackMemoryMisaligned);
        }
        let stack_len = words
            .len()
            .checked_div(stack_ratio)
            .ok_or(MachineError::StackMemoryMisaligned)?;
        let stack_ptr = ptr as *mut StackWord;
        // SAFETY: caller ensures alignment; length is computed from input slice.
        let stack = unsafe { core::slice::from_raw_parts_mut(stack_ptr, stack_len) };
        Ok(Self { data: stack, len: 0 })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push(&mut self, value: StackWord) -> Result<(), MachineError> {
        if self.len >= self.data.len() {
            return Err(MachineError::StackOverflow);
        }
        let slot = self
            .data
            .get_mut(self.len)
            .ok_or(MachineError::StackOverflow)?;
        *slot = value;
        self.len = self
            .len
            .checked_add(1)
            .ok_or(MachineError::StackOverflow)?;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<StackWord> {
        if self.len == 0 {
            return None;
        }
        self.len = self.len.checked_sub(1)?;
        self.data.get(self.len).copied()
    }

    pub fn last(&self) -> Option<&StackWord> {
        if self.len == 0 {
            return None;
        }
        let index = self.len.checked_sub(1)?;
        self.data.get(index)
    }

    pub fn get(&self, index: usize) -> Option<&StackWord> {
        if index >= self.len {
            return None;
        }
        self.data.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut StackWord> {
        if index >= self.len {
            return None;
        }
        self.data.get_mut(index)
    }

    pub fn insert(&mut self, index: usize, value: StackWord) -> Result<(), MachineError> {
        if index > self.len {
            return Err(MachineError::StackUnderFlow);
        }
        if self.len >= self.data.len() {
            return Err(MachineError::StackOverflow);
        }
        let insert_at = index
            .checked_add(1)
            .ok_or(MachineError::StackOverflow)?;
        self.data.copy_within(index..self.len, insert_at);
        let slot = self
            .data
            .get_mut(index)
            .ok_or(MachineError::StackOverflow)?;
        *slot = value;
        self.len = self
            .len
            .checked_add(1)
            .ok_or(MachineError::StackOverflow)?;
        Ok(())
    }

    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len {
            self.len = new_len;
        }
    }

    pub fn as_slice(&self) -> &[StackWord] {
        self.data.get(..self.len).unwrap_or(&[])
    }
}

#[allow(clippy::indexing_slicing)]
impl core::ops::Index<usize> for StackSlice<'_> {
    type Output = StackWord;
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

#[allow(clippy::indexing_slicing)]
impl core::ops::IndexMut<usize> for StackSlice<'_> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let len = self.len;
        &mut self.data[..len][index]
    }
}

fn stack_bottom_for_globals(globals_size: ProgramWord) -> usize {
    let globals_size = usize::from(globals_size);
    let stack_ratio = size_of::<StackWord>()
        .checked_div(size_of::<ProgramWord>())
        .unwrap_or(1);
    if stack_ratio == 0 {
        return globals_size;
    }
    let rem = globals_size
        .checked_rem(stack_ratio)
        .unwrap_or(0);
    if rem == 0 {
        globals_size
    } else {
        let bump = stack_ratio.saturating_sub(rem);
        globals_size.saturating_add(bump)
    }
}

fn get_mut_or<E>(slice: &mut [ProgramWord], index: usize, err: E) -> Result<&mut ProgramWord, E> {
    slice.get_mut(index).ok_or(err)
}

fn set_value<E>(
    slice: &mut [ProgramWord],
    index: usize,
    value: ProgramWord,
    err: E,
) -> Result<(), E> {
    *get_mut_or(slice, index, err)? = value;
    Ok(())
}

#[repr(u16)] // Must match ProgramWord
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
    LocalLoad,
    LocalStore,
    GlobalLoad,
    GlobalStore,
    LoadStatic,
    Jump,
    Exit,
    Call,
    CallShared,
    StackLoad,
    StackStore,
    Dup,
    Swap,
    Return,
}

impl From<Ops> for ProgramWord {
    fn from(op: Ops) -> ProgramWord {
        op as ProgramWord
    }
}

impl TryFrom<ProgramWord> for Ops {
    type Error = MachineError;
    fn try_from(value: ProgramWord) -> Result<Self, Self::Error> {
        // SAFTY: we require Ops to be in range of u16
        // with `repr` macro.
        if value >= Ops::VARIANT_COUNT as u16 {
            return Err(MachineError::InvalidOp(value));
        }

        // SAFTY: We just check that the value is in range.
        let op = unsafe { transmute::<ProgramWord, Self>(value) };
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
    InvalidOp(ProgramWord),
    #[error("the pop op code was executed on an empty stack")]
    PopOnEmptyStack,
    #[error("attempted opperation would overflow the stack")]
    StackOverflow,
    #[error("attempted opperation would underflow the stack")]
    StackUnderFlow,
    #[error("there are not enogh arguments to call the function")]
    TwoFewArguments,
    #[error("word {0} out of range of color value")]
    ColorOutOfRange(StackWord),
    #[error("indesx {0} out of range of static data")]
    OutOfBoudsStaticRead(usize),
    #[error("index {0} out of range of static data")]
    GlobalsBufferTooSmall(ProgramWord),
    #[error("index {0} out of range for machine index")]
    MachineIndexOutOfRange(ProgramWord),
    #[error("index {0} out of range for shared function index")]
    SharedFunctionIndexOutOfRange(ProgramWord),
    #[error("shared globals access out of range: {0}")]
    SharedGlobalsAccessOutOfRange(ProgramWord),
    #[error("stack value {0} does not fit in a program word")]
    StackValueTooLargeForProgramWord(StackWord),
    #[error("stack value {0} does not fit in usize")]
    StackValueTooLargeForUsize(StackWord),
    #[error("program version {0} is not supported")]
    InvalidProgramVersion(ProgramWord),
    #[error("memory buffer too small (needed {needed}, provided {provided})")]
    MemoryBufferTooSmall { needed: usize, provided: usize },
    #[error("stack memory is not aligned for StackWord")]
    StackMemoryMisaligned,
}

pub const PROGRAM_VERSION: ProgramWord = 2;
pub const VERSION_OFFSET: usize = 0;
pub const MACHINE_COUNT_OFFSET: usize = VERSION_OFFSET + 1;
pub const GLOBALS_SIZE_OFFSET: usize = MACHINE_COUNT_OFFSET + 1;
pub const SHARED_FUNCTION_COUNT_OFFSET: usize = GLOBALS_SIZE_OFFSET + 1;
pub const TYPE_COUNT_OFFSET: usize = SHARED_FUNCTION_COUNT_OFFSET + 1;
pub const INSTANCE_TABLE_OFFSET: usize = TYPE_COUNT_OFFSET + 1;
pub const TYPE_TABLE_OFFSET: usize = INSTANCE_TABLE_OFFSET + 1;
pub const SHARED_FUNCTION_TABLE_OFFSET: usize = TYPE_TABLE_OFFSET + 1;
pub const HEADER_WORDS: usize = SHARED_FUNCTION_TABLE_OFFSET + 1;

const INIT_OFFSET: usize = 0;
const GET_COLOR_OFFSET: usize = INIT_OFFSET + 1;

#[derive(Debug)]
pub struct MachineTypeDescriptor<const FUNCTION_COUNT_MAX: usize> {
    pub functions: Vec<FunctionIndex, FUNCTION_COUNT_MAX>,
    pub globals_size: ProgramWord,
}

impl<const FUNCTION_COUNT_MAX: usize> MachineTypeDescriptor<FUNCTION_COUNT_MAX> {
    pub fn new(globals_size: ProgramWord) -> Self {
        Self {
            functions: Vec::new(),
            globals_size,
        }
    }

    pub fn add_function(&mut self, index: FunctionIndex) -> Result<(), FunctionIndex> {
        self.functions.push(index)
    }
}

impl<const FUNCTION_COUNT_MAX: usize> Default for MachineTypeDescriptor<FUNCTION_COUNT_MAX> {
    fn default() -> Self {
        Self::new(0)
    }
}

#[derive(Debug)]
pub struct MachineInstanceDescriptor {
    pub type_id: ProgramWord,
    pub globals_base: ProgramWord,
}

#[derive(Debug)]
pub struct ProgramDescriptor<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    pub length: usize,
    pub types: Vec<MachineTypeDescriptor<FUNCTION_COUNT_MAX>, MACHINE_COUNT_MAX>,
    pub instances: Vec<MachineInstanceDescriptor, MACHINE_COUNT_MAX>,
}

impl<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new() -> Self {
        Self {
            length: 0,
            types: Vec::new(),
            instances: Vec::new(),
        }
    }

    pub fn add_type(
        &mut self,
        machine_descriptor: MachineTypeDescriptor<FUNCTION_COUNT_MAX>,
    ) -> Result<(), MachineTypeDescriptor<FUNCTION_COUNT_MAX>> {
        self.types.push(machine_descriptor)
    }

    pub fn add_instance(
        &mut self,
        instance: MachineInstanceDescriptor,
    ) -> Result<(), MachineInstanceDescriptor> {
        self.instances.push(instance)
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
    static_data: &'a [ProgramWord],
    globals: &'b mut [ProgramWord],
    stack: StackSlice<'b>,
    frame_pointer: StackWord,
    locals_base: ProgramWord,
}

impl<'a, 'b> Program<'a, 'b> {
    pub fn new(
        static_data: &'a [ProgramWord],
        memory: &'b mut [ProgramWord],
    ) -> Result<Self, MachineError> {
        let Some(version) = static_data.get(VERSION_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(VERSION_OFFSET));
        };
        if *version != PROGRAM_VERSION {
            return Err(MachineError::InvalidProgramVersion(*version));
        }
        let Some(globals_size) = static_data.get(GLOBALS_SIZE_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(GLOBALS_SIZE_OFFSET));
        };

        if *globals_size as usize > memory.len() {
            return Err(MachineError::GlobalsBufferTooSmall(*globals_size));
        }
        let memory = ProgramMemory::split(memory, *globals_size)?;

        Ok(Self {
            static_data,
            globals: memory.globals,
            stack: memory.stack,
            frame_pointer: 0,
            locals_base: 0,
        })
    }


    pub fn machine_count(&self) -> Result<ProgramWord, MachineError> {
        let Some(count) = self.static_data.get(MACHINE_COUNT_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(MACHINE_COUNT_OFFSET));
        };

        Ok(*count)
    }

    pub fn type_count(&self) -> Result<ProgramWord, MachineError> {
        let Some(count) = self.static_data.get(TYPE_COUNT_OFFSET) else {
            return Err(MachineError::OutOfBoudsStaticRead(TYPE_COUNT_OFFSET));
        };

        Ok(*count)
    }

    pub fn shared_function_count(&self) -> Result<ProgramWord, MachineError> {
        let Some(count) = self
            .static_data
            .get(SHARED_FUNCTION_COUNT_OFFSET)
        else {
            return Err(MachineError::OutOfBoudsStaticRead(
                SHARED_FUNCTION_COUNT_OFFSET,
            ));
        };
        Ok(*count)
    }

    fn instance_table_offset(&self) -> Result<usize, MachineError> {
        let offset = read_static(INSTANCE_TABLE_OFFSET, self.static_data)?;
        Ok(offset as usize)
    }

    fn type_table_offset(&self) -> Result<usize, MachineError> {
        let offset = read_static(TYPE_TABLE_OFFSET, self.static_data)?;
        Ok(offset as usize)
    }

    fn shared_function_table_offset(&self) -> Result<usize, MachineError> {
        let offset = read_static(SHARED_FUNCTION_TABLE_OFFSET, self.static_data)?;
        Ok(offset as usize)
    }

    fn instance_globals_offset(
        &self,
        machine_number: ProgramWord,
    ) -> Result<ProgramWord, MachineError> {
        let machine_count = self.machine_count()?;
        if machine_number >= machine_count {
            return Err(MachineError::MachineIndexOutOfRange(machine_number));
        };
        let table_offset = self.instance_table_offset()?;
        let entry_index = (machine_number as usize)
            .checked_mul(2)
            .and_then(|offset| table_offset.checked_add(offset))
            .ok_or(MachineError::OutOfBoudsStaticRead(table_offset))?;
        let globals_base_index = entry_index
            .checked_add(1)
            .ok_or(MachineError::OutOfBoudsStaticRead(entry_index))?;
        read_static(globals_base_index, self.static_data)
    }

    fn get_type_for_instance(
        &self,
        machine_number: ProgramWord,
    ) -> Result<ProgramWord, MachineError> {
        let machine_count = self.machine_count()?;
        if machine_number >= machine_count {
            return Err(MachineError::MachineIndexOutOfRange(machine_number));
        };
        let table_offset = self.instance_table_offset()?;
        let entry_index = (machine_number as usize)
            .checked_mul(2)
            .and_then(|offset| table_offset.checked_add(offset))
            .ok_or(MachineError::OutOfBoudsStaticRead(table_offset))?;
        read_static(entry_index, self.static_data)
    }

    fn get_type_function_entry(
        &self,
        type_id: ProgramWord,
        function_number: usize,
    ) -> Result<usize, MachineError> {
        let type_count = self.type_count()?;
        if type_id >= type_count {
            return Err(MachineError::MachineIndexOutOfRange(type_id));
        };
        let type_table_offset = self.type_table_offset()?;
        let type_entry_index = (type_id as usize)
            .checked_mul(2)
            .and_then(|offset| type_table_offset.checked_add(offset))
            .ok_or(MachineError::OutOfBoudsStaticRead(type_table_offset))?;
        let function_count = read_static(type_entry_index, self.static_data)? as usize;
        if function_number >= function_count {
            return Err(MachineError::SharedFunctionIndexOutOfRange(
                function_number as ProgramWord,
            ));
        }
        let function_table_offset = read_static(
            type_entry_index
                .checked_add(1)
                .ok_or(MachineError::OutOfBoudsStaticRead(type_entry_index))?,
            self.static_data,
        )? as usize;
        let entry_index = function_table_offset
            .checked_add(function_number)
            .ok_or(MachineError::OutOfBoudsStaticRead(function_table_offset))?;
        let entry_point = read_static(entry_index, self.static_data)?;
        Ok(entry_point as usize)
    }

    fn get_function_entry(
        &self,
        machine_number: ProgramWord,
        function_number: usize,
    ) -> Result<usize, MachineError> {
        let type_id = self.get_type_for_instance(machine_number)?;
        self.get_type_function_entry(type_id, function_number)
    }

    fn get_shared_function_entry(
        &self,
        function_number: ProgramWord,
    ) -> Result<usize, MachineError> {
        let shared_function_count = self.shared_function_count()?;
        if function_number >= shared_function_count {
            return Err(MachineError::SharedFunctionIndexOutOfRange(
                function_number,
            ));
        }
        let table_offset = self.shared_function_table_offset()?;
        let entry_index = table_offset
            .checked_add(function_number as usize)
            .ok_or(MachineError::OutOfBoudsStaticRead(table_offset))?;
        let entry_point = read_static(entry_index, self.static_data)?;
        Ok(entry_point as usize)
    }

    pub fn stack(&self) -> &StackSlice<'b> {
        &self.stack
    }

    pub fn stack_mut(&mut self) -> &mut StackSlice<'b> {
        &mut self.stack
    }

    pub fn init_machine(
        &mut self,
        machine_number: ProgramWord,
    ) -> Result<(), MachineError> {
        let entry_point = self.get_function_entry(machine_number, INIT_OFFSET)?;
        self.run(machine_number, entry_point)?;
        Ok(())
    }

    pub fn get_led_color(
        &mut self,
        machine_number: ProgramWord,
        index: u16,
        tick: u32,
    ) -> Result<(u8, u8, u8), MachineError> {
        if self.stack().len() < 3 {
            return Err(MachineError::TwoFewArguments);
        }
        self.stack_mut().push(StackWord::from(index))?;
        self.stack_mut().push(StackWord::from(tick))?;


        let entry_point = self.get_function_entry(machine_number, GET_COLOR_OFFSET)?;
        self.run(machine_number, entry_point)?;

        let Some(blue) = self.stack_mut().pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(green) = self.stack_mut().pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let Some(red) = self.stack_mut().pop() else {
            return Err(MachineError::StackUnderFlow);
        };

        let red = word_to_color(red)?;
        let green = word_to_color(green)?;
        let blue = word_to_color(blue)?;

        Ok((red, green, blue))
    }

    pub fn call(
        &mut self,
        machine_number: ProgramWord,
        function_number: usize,
    ) -> Result<(), MachineError> {
        let entry_point = self.get_function_entry(machine_number, function_number)?;

        self.run(machine_number, entry_point)?;
        Ok(())
    }

    fn run(
        &mut self,
        machine_number: ProgramWord,
        entry_point: usize,
    ) -> Result<(), MachineError> {
        let mut pc = entry_point;
        let locals_base = self.instance_globals_offset(machine_number)?;
        self.locals_base = locals_base;
        let stack_ptr = core::ptr::addr_of_mut!(self.stack);

        loop {
            let word = read_static(pc, self.static_data)?;
            let op = word.try_into()?;
            match op {
                Ops::Pop => {
                    let stack = unsafe { &mut *stack_ptr };
                    if stack.pop().is_none() {
                        return Err(MachineError::PopOnEmptyStack);
                    }
                }
                Ops::Push => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, program_word_to_stack(word))?;
                }
                Ops::BranchLessThan => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    if lhs < rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchLessThanEq => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    if lhs <= rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchGreaterThan => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    if lhs > rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchGreaterThanEq => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    if lhs >= rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::BranchEqual => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    if lhs == rhs {
                        pc = target;
                        continue;
                    }
                }
                Ops::And => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let result = if lhs != 0 && rhs != 0 { 1 } else { 0 };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::Or => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let result = if lhs != 0 || rhs != 0 { 1 } else { 0 };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::Xor => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let result = if (lhs != 0) ^ (rhs != 0) { 1 } else { 0 };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::Not => {
                    let value = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop(stack)?
                    };
                    let result = if value == 0 { 1 } else { 0 };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::BitwiseAnd => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs & rhs)?;
                }
                Ops::BitwiseOr => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs | rhs)?;
                }
                Ops::BitwiseXor => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs ^ rhs)?;
                }
                Ops::BitwiseNot => {
                    let value = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, !value)?;
                }
                Ops::Multiply => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs.wrapping_mul(rhs))?;
                }
                Ops::Divide => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let result = lhs
                        .checked_div(rhs)
                        .ok_or(MachineError::InvalidOp(word))?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::Mod => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let result = lhs
                        .checked_rem(rhs)
                        .ok_or(MachineError::InvalidOp(word))?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, result)?;
                }
                Ops::Add => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs.wrapping_add(rhs))?;
                }
                Ops::Subtract => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, lhs.wrapping_sub(rhs))?;
                }
                Ops::LocalLoad => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<ProgramWord>() <= size_of::<usize>());
                    }
                    let index = self
                        .locals_base
                        .checked_add(offset)
                        .ok_or(MachineError::OutOfBoundsGlobalsAccess(
                            usize::from(self.locals_base),
                        ))?;
                    // SAFTY: const assersion prouves this is safe
                    let index = index as usize;

                    let word = read_global(index, self.globals)?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, program_word_to_stack(word))?;
                }
                Ops::LocalStore => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;

                    const { assert!(size_of::<ProgramWord>() <= size_of::<usize>()) }
                    let index = self
                        .locals_base
                        .checked_add(offset)
                        .ok_or(MachineError::OutOfBoundsGlobalsAccess(
                            usize::from(self.locals_base),
                        ))?;
                    // SAFTY: const assersion prouves this is safe
                    let index = index as usize;

                    let word = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program(pop(stack)?)?
                    };

                    set_value(
                        self.globals,
                        index,
                        word,
                        MachineError::OutOfBoundsGlobalsAccess(index),
                    )?;
                }
                Ops::GlobalLoad => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;

                    const {
                        assert!(size_of::<ProgramWord>() <= size_of::<usize>());
                    }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    let word = read_global(index, self.globals)?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, program_word_to_stack(word))?;
                }
                Ops::GlobalStore => {
                    pc = next_pc(pc)?;
                    let word = read_static(pc, self.static_data)?;

                    const { assert!(size_of::<ProgramWord>() <= size_of::<usize>()) }
                    // SAFTY: const assersion prouves this is safe
                    let index = word as usize;

                    let word = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program(pop(stack)?)?
                    };

                    set_value(
                        self.globals,
                        index,
                        word,
                        MachineError::OutOfBoundsGlobalsAccess(index),
                    )?;
                }
                Ops::LoadStatic => {
                    let addr = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    let value = read_static(addr, self.static_data)?;
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, program_word_to_stack(value))?;
                }
                Ops::Jump => {
                    let target = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack_word_to_program_index(pop(stack)?)?
                    };
                    pc = target;
                    continue;
                }
                Ops::StackLoad => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;
                    let frame_pointer = stack_word_to_usize(self.frame_pointer)?;
                    let offset = usize::from(offset);
                    let index = frame_pointer
                        .checked_add(offset)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let value = {
                        let stack = unsafe { &mut *stack_ptr };
                        *stack.get(index).ok_or(MachineError::StackUnderFlow)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, value)?;
                }
                Ops::StackStore => {
                    pc = next_pc(pc)?;
                    let offset = read_static(pc, self.static_data)?;
                    let frame_pointer = stack_word_to_usize(self.frame_pointer)?;
                    let offset = usize::from(offset);
                    let index = frame_pointer
                        .checked_add(offset)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let value = {
                        let stack = unsafe { &mut *stack_ptr };
                        *stack.last().ok_or(MachineError::StackUnderFlow)?
                    };
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        let slot = stack
                            .get_mut(index)
                            .ok_or(MachineError::StackUnderFlow)?;
                        *slot = value;
                    }
                    let _ = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop(stack)?
                    };
                }
                Ops::Dup => {
                    let value = {
                        let stack = unsafe { &mut *stack_ptr };
                        *stack.last().ok_or(MachineError::StackUnderFlow)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, value)?;
                }
                Ops::Swap => {
                    let (lhs, rhs) = {
                        let stack = unsafe { &mut *stack_ptr };
                        pop2(stack)?
                    };
                    let stack = unsafe { &mut *stack_ptr };
                    push(stack, rhs)?;
                    push(stack, lhs)?;
                }
                Ops::Exit => {
                    self.frame_pointer = 0;
                    break
                }
                Ops::Call => {
                    // Stack convention: ... args, arg_count, func_index
                    let (function_index, _arg_count, arg_start) = {
                        let stack = unsafe { &mut *stack_ptr };
                        let function_index =
                            usize::from(stack_word_to_program(pop(stack)?)?);
                        let arg_count = stack_word_to_usize(pop(stack)?)?;
                        let arg_start = stack
                            .len()
                            .checked_sub(arg_count)
                            .ok_or(MachineError::StackUnderFlow)?;
                        (function_index, arg_count, arg_start)
                    };
                    // Save current frame pointer so the callee can access its caller frame.
                    let saved_frame_pointer = self.frame_pointer;
                    // Precompute return PC so it can be pushed ahead of the callee's args.
                    let return_pc = ProgramWord::try_from(next_pc(pc)?)
                        .map_err(|_| MachineError::StackOverflow)?;
                    let return_pc = program_word_to_stack(return_pc);
                    // Insert return PC before the first argument for this call frame layout:
                    // [return_pc, saved_fp, arg0, arg1, ...]
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        stack
                            .insert(arg_start, return_pc)
                            .map_err(|_| MachineError::StackOverflow)?;
                    }
                    // Insert saved FP immediately after return PC.
                    let saved_pointer_index = arg_start
                        .checked_add(1)
                        .ok_or(MachineError::StackOverflow)?;
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        stack
                            .insert(saved_pointer_index, saved_frame_pointer)
                            .map_err(|_| MachineError::StackOverflow)?;
                    }
                    // Frame pointer points at arg0, which is now shifted by two slots.
                    let new_frame_pointer = arg_start
                        .checked_add(2)
                        .ok_or(MachineError::StackOverflow)?;
                    // Convert usize->StackWord safely; StackWord limits keep stack indexing bounded.
                    let new_frame_pointer =
                        StackWord::try_from(new_frame_pointer)
                            .map_err(|_| MachineError::StackOverflow)?;
                    self.frame_pointer = new_frame_pointer;
                    let entry_point = self.get_function_entry(machine_number, function_index)?;
                    self.run(machine_number, entry_point)?;
                    // Restore caller's frame pointer after returning.
                    self.frame_pointer = saved_frame_pointer;
                    pc = stack_word_to_program_index(return_pc)?;
                    continue;
                }
                Ops::CallShared => {
                    // Stack convention: ... args, arg_count, shared_func_index
                    let (function_index, _arg_count, arg_start) = {
                        let stack = unsafe { &mut *stack_ptr };
                        let function_index =
                            stack_word_to_program(pop(stack)?)?;
                        let arg_count = stack_word_to_usize(pop(stack)?)?;
                        let arg_start = stack
                            .len()
                            .checked_sub(arg_count)
                            .ok_or(MachineError::StackUnderFlow)?;
                        (function_index, arg_count, arg_start)
                    };
                    let saved_frame_pointer = self.frame_pointer;
                    let return_pc = ProgramWord::try_from(next_pc(pc)?)
                        .map_err(|_| MachineError::StackOverflow)?;
                    let return_pc = program_word_to_stack(return_pc);
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        stack
                            .insert(arg_start, return_pc)
                            .map_err(|_| MachineError::StackOverflow)?;
                    }
                    let saved_pointer_index = arg_start
                        .checked_add(1)
                        .ok_or(MachineError::StackOverflow)?;
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        stack
                            .insert(saved_pointer_index, saved_frame_pointer)
                            .map_err(|_| MachineError::StackOverflow)?;
                    }
                    let new_frame_pointer = arg_start
                        .checked_add(2)
                        .ok_or(MachineError::StackOverflow)?;
                    let new_frame_pointer =
                        StackWord::try_from(new_frame_pointer)
                            .map_err(|_| MachineError::StackOverflow)?;
                    self.frame_pointer = new_frame_pointer;
                    let entry_point = self.get_shared_function_entry(function_index)?;
                    self.run(machine_number, entry_point)?;
                    self.frame_pointer = saved_frame_pointer;
                    pc = stack_word_to_program_index(return_pc)?;
                    continue;
                }
                Ops::Return => {
                    // Read the return count operand that follows RET.
                    pc = next_pc(pc)?;
                    let return_count = usize::from(read_static(pc, self.static_data)?);
                    // Compute frame metadata positions relative to the current frame pointer.
                    let fp_index = stack_word_to_usize(self.frame_pointer)?;
                    let return_pc_index = fp_index
                        .checked_sub(2)
                        .ok_or(MachineError::StackUnderFlow)?;
                    let saved_fp_index = fp_index
                        .checked_sub(1)
                        .ok_or(MachineError::StackUnderFlow)?;
                    // Fetch return PC and the caller's frame pointer from the frame header.
                    let return_pc = {
                        let stack = unsafe { &mut *stack_ptr };
                        *stack
                            .get(return_pc_index)
                            .ok_or(MachineError::StackUnderFlow)?
                    };
                    let saved_frame_pointer = {
                        let stack = unsafe { &mut *stack_ptr };
                        *stack
                            .get(saved_fp_index)
                            .ok_or(MachineError::StackUnderFlow)?
                    };
                    // Copy return values from the top of the stack before unwinding the frame.
                    let original_len = {
                        let stack = unsafe { &mut *stack_ptr };
                        stack.len()
                    };
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
                        let value = {
                            let stack = unsafe { &mut *stack_ptr };
                            *stack
                                .get(src_index)
                                .ok_or(MachineError::StackUnderFlow)?
                        };
                        {
                            let stack = unsafe { &mut *stack_ptr };
                            let slot = stack
                                .get_mut(dest_index)
                                .ok_or(MachineError::StackUnderFlow)?;
                            *slot = value;
                        }
                    }

                    // Drop the call frame header and locals, keeping only the return values.
                    let new_len = return_pc_index
                            .checked_add(return_count)
                            .ok_or(MachineError::StackUnderFlow)?;
                    {
                        let stack = unsafe { &mut *stack_ptr };
                        stack.truncate(new_len);
                    }
                    // Restore caller frame pointer and jump to saved return PC.
                    self.frame_pointer = saved_frame_pointer;
                    pc = stack_word_to_program_index(return_pc)?;
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

fn read_static(index: usize, program: &[ProgramWord]) -> Result<ProgramWord, MachineError> {
    match program.get(index) {
        None => Err(MachineError::OutOfBoudsStaticRead(index)),
        Some(word) => Ok(*word),
    }
}

fn read_global(index: usize, globals: &[ProgramWord]) -> Result<ProgramWord, MachineError> {
    match globals.get(index) {
        None => Err(MachineError::OutOfBoundsGlobalsAccess(index)),
        Some(word) => Ok(*word),
    }
}

fn pop(
    stack: &mut StackSlice<'_>,
) -> Result<StackWord, MachineError> {
    stack.pop().ok_or(MachineError::StackUnderFlow)
}

fn pop2(
    stack: &mut StackSlice<'_>,
) -> Result<(StackWord, StackWord), MachineError> {
    let rhs = pop(stack)?;
    let lhs = pop(stack)?;
    Ok((lhs, rhs))
}

fn push(
    stack: &mut StackSlice<'_>,
    value: StackWord,
) -> Result<(), MachineError> {
    stack.push(value)
}

fn word_to_color(word: StackWord) -> Result<u8, MachineError> {
    match u8::try_from(word) {
        Ok(c) => Ok(c),
        Err(_) => Err(MachineError::ColorOutOfRange(word)),
    }
}

fn program_word_to_stack(word: ProgramWord) -> StackWord {
    StackWord::from(word)
}

fn stack_word_to_program(value: StackWord) -> Result<ProgramWord, MachineError> {
    ProgramWord::try_from(value).map_err(|_| MachineError::StackValueTooLargeForProgramWord(value))
}

fn stack_word_to_usize(value: StackWord) -> Result<usize, MachineError> {
    usize::try_from(value).map_err(|_| MachineError::StackValueTooLargeForUsize(value))
}

fn stack_word_to_program_index(value: StackWord) -> Result<usize, MachineError> {
    let program_word = stack_word_to_program(value)?;
    Ok(usize::from(program_word))
}

#[cfg(test)]
mod test;
