use super::*;

#[derive(Error, Debug)]
pub enum MachineBuilderError {
    BufferTooSmall,
    MachineCountOverflowsWord(usize),
    TooLarge(usize),
    FunctionCoutExceeded,
    GlobalOutOfRange(ProgramWord),
    MachineCountExceeded,
}

/// Index for static data.
pub struct DataIndex(ProgramWord);

/// Index for function.
#[derive(Clone, Debug)]
pub struct FunctionIndex(ProgramWord);

impl FunctionIndex {
    pub fn new(index: ProgramWord) -> Self {
        Self(index)
    }

    pub fn to_word(&self) -> ProgramWord {
        self.0
    }
}

impl From<FunctionIndex> for u32 {
    fn from(val: FunctionIndex) -> Self {
        val.0 as u32
    }
}

/// Program is
/// [machine_count][machine offsets..][machines ...]
///
pub struct ProgramBuilder<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    buffer: &'a mut [ProgramWord],
    next_machine_builder: ProgramWord,
    free: ProgramWord,
    descriptor: ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    shared_globals_size: ProgramWord,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new(buffer: &'a mut [ProgramWord], machine_count: ProgramWord) -> Result<Self, MachineBuilderError> {
        // Assure `Words` can be cast to `usize`
        const { assert!(size_of::<ProgramWord>() <= size_of::<usize>()) };

        // Make sure we have at least enough space to strore
        // the offsets to each machine and the count of machines.
        // NOTE: we could probbly make some assumption about
        // the smallest usafal machine and ensure we have room
        // for that too.
        let Some(required) = (machine_count as usize).checked_add(2) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                machine_count as usize,
            ));
        };
        if buffer.len() <= required {
            return Err(MachineBuilderError::BufferTooSmall);
        }

        set_value(buffer, MACHINE_COUNT_OFFSET, 0, MachineBuilderError::BufferTooSmall)?; // Machine count
        set_value(buffer, GLOBALS_SIZE_OFFSET, 0, MachineBuilderError::BufferTooSmall)?; // Globals size
        let Some(free) = machine_count.checked_add(2) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                machine_count as usize,
            ));
        };
        Ok(Self {
            buffer,
            free,
            next_machine_builder: 0,
            descriptor: ProgramDescriptor::new(),
            shared_globals_size: 0,
        })
    }

    pub fn set_shared_globals_size(&mut self, shared_globals_size: ProgramWord) -> Result<(), MachineBuilderError> {
        if self.next_machine_builder != 0 {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        set_value(
            self.buffer,
            GLOBALS_SIZE_OFFSET,
            shared_globals_size,
            MachineBuilderError::BufferTooSmall,
        )?;
        self.shared_globals_size = shared_globals_size;
        Ok(())
    }

    pub fn new_machine(
        self,
        function_count: ProgramWord,
        globals_size: ProgramWord,
    ) -> Result<MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, MachineBuilderError>
    {
        let Some(next_machine_builder) = self.next_machine_builder.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_machine_builder as usize,
            ));
        };
        set_value(
            self.buffer,
            MACHINE_COUNT_OFFSET,
            next_machine_builder,
            MachineBuilderError::BufferTooSmall,
        )?;
        let Some(machine_table_index) = (self.next_machine_builder as usize)
            .checked_add(MACHINE_TABLE_OFFSET)
        else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        set_value(
            self.buffer,
            machine_table_index,
            self.free as ProgramWord,
            MachineBuilderError::BufferTooSmall,
        )?;
        let globals_offset = *self
            .buffer
            .get(GLOBALS_SIZE_OFFSET)
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        let shared_globals_size = self.shared_globals_size;
        MachineBuilder::new(
            self,
            function_count,
            globals_size,
            globals_offset,
            shared_globals_size,
        )
    }

    fn allocate(&mut self, word_count: ProgramWord) -> Result<(), MachineBuilderError> {
        let free = usize::from(self.free);
        let word_count = usize::from(word_count);
        let Some(end) = free.checked_add(word_count) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        if end > self.buffer.len() {
            return Err(MachineBuilderError::BufferTooSmall);
        }
        let Some(new_free) = self.free.checked_add(word_count as ProgramWord) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        self.free = new_free;
        Ok(())
    }

    fn add_word(&mut self, word: ProgramWord) -> Result<(), MachineBuilderError> {
        let index = usize::from(self.free);
        set_value(
            self.buffer,
            index,
            word,
            MachineBuilderError::BufferTooSmall,
        )?;
        let Some(new_free) = self.free.checked_add(1) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        self.free = new_free;
        Ok(())
    }

    fn finish_machine(&mut self, globals_size: ProgramWord, machine_descriptor: MachineDescriptor<FUNCTION_COUNT_MAX>) -> Result<(), MachineBuilderError>{
        if self.descriptor.add_machine(machine_descriptor).is_err() {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        let globals_slot = get_mut_or(
            self.buffer,
            GLOBALS_SIZE_OFFSET,
            MachineBuilderError::BufferTooSmall,
        )?;
        let Some(new_globals_size) = globals_slot.checked_add(globals_size) else {
            return Err(MachineBuilderError::TooLarge(globals_size as usize));
        };
        *globals_slot = new_globals_size;
        let Some(next_machine_builder) = self.next_machine_builder.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_machine_builder as usize,
            ));
        };
        self.next_machine_builder = next_machine_builder;
        Ok(())
    }

    pub fn finish_program(self) -> ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX> {
        let mut descriptor = self.descriptor;
        descriptor.length = self.free as usize;
        descriptor
    }
}

/// Machine format is:
/// [globals size][globals offset][function offsets...][static data + functions...]
pub struct MachineBuilder<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    program: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    machine_start: ProgramWord,
    function_count: ProgramWord,
    next_function_number: ProgramWord,
    globals_size: ProgramWord,
    globals_offset: ProgramWord,
    shared_globals_size: ProgramWord,
    discriptor: MachineDescriptor<FUNCTION_COUNT_MAX>,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn program_free(&self) -> ProgramWord {
        self.program.free
    }

    fn patch_word(&mut self, index: ProgramWord, value: ProgramWord) -> Result<(), MachineBuilderError> {
        let index = usize::from(index);
        let Some(slot) = self.program.buffer.get_mut(index) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        *slot = value;
        Ok(())
    }
    pub fn new(
        mut program: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        function_count: ProgramWord,
        globals_size: ProgramWord,
        globals_offset: ProgramWord,
        shared_globals_size: ProgramWord,
    ) -> Result<Self, MachineBuilderError> {
        let machine_start = program.free;
        program.add_word(globals_size)?;
        program.add_word(globals_offset)?;
        program.allocate(function_count)?;
        Ok(Self {
            program,
            machine_start,
            function_count,
            next_function_number: 0,
            globals_size,
            globals_offset,
            shared_globals_size,
            discriptor: MachineDescriptor::new(),
        })
    }

    pub fn add_static(&mut self, data: &[ProgramWord]) -> Result<DataIndex, MachineBuilderError> {
        if data.len() >= ProgramWord::MAX as usize {
            return Err(MachineBuilderError::TooLarge(data.len()));
        }
        let size = data.len() as ProgramWord;
        let index = DataIndex(self.program.free);
        self.program.allocate(size)?;
        let start = usize::from(index.0);
        let end = start
            .checked_add(usize::from(size))
            .ok_or(MachineBuilderError::BufferTooSmall)?; //BUG: this is the worng error. Sould be an overflow error.
        let Some(target) = self.program.buffer.get_mut(start..end) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        target.copy_from_slice(data);
        Ok(index)
    }

    pub fn reserve_function(&mut self) -> Result<FunctionIndex, MachineBuilderError> {
        if self.next_function_number >= self.function_count {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        let index = FunctionIndex(self.next_function_number);
        let Some(next_function_number) = self.next_function_number.checked_add(1) else {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        };
        self.next_function_number = next_function_number;
        Ok(index)
    }

    pub fn new_function(
        mut self,
    ) -> Result<FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, MachineBuilderError>
    {
        let index = self.reserve_function()?;
        Ok(FunctionBuilder::new(self, index))
    }

    pub fn new_function_at_index(
        self,
        index: FunctionIndex,
    ) -> Result<FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, MachineBuilderError>
    {
        Ok(FunctionBuilder::new(self, index))
    }

    pub fn finish(mut self) -> Result<ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, MachineBuilderError> {
        self.program.finish_machine(self.globals_size, self.discriptor)?;
        Ok(self.program)
    }

    pub fn globals_offset(&self) -> ProgramWord {
        self.globals_offset
    }

    fn validate_global_address(&self, address: ProgramWord) -> Result<(), MachineBuilderError> {
        if address < self.shared_globals_size {
            return Ok(());
        }
        let Some(end) = self.globals_offset.checked_add(self.globals_size) else {
            return Err(MachineBuilderError::GlobalOutOfRange(address));
        };
        if address < self.globals_offset || address >= end {
            return Err(MachineBuilderError::GlobalOutOfRange(address));
        }
        Ok(())
    }

    fn add_word(&mut self, word: ProgramWord) -> Result<(), MachineBuilderError> {
        self.program.add_word(word)
    }

    fn finish_function(
        &mut self,
        function_start: ProgramWord,
        index: FunctionIndex,
    ) -> Result<(), MachineBuilderError> {
        if self.discriptor.add_function(index.clone()).is_err() {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        let base = usize::from(self.machine_start);
        let Some(machine_offset) = base.checked_add(MACHINE_FUNCTIONS_OFFSET) else {
            return Err(MachineBuilderError::BufferTooSmall); // BUG: wrong error
        };
        let Some(index) = machine_offset.checked_add(usize::from(index.0)) else {
            return Err(MachineBuilderError::BufferTooSmall); // BUG: worng error
        };
        set_value(
            self.program.buffer,
            index,
            function_start,
            MachineBuilderError::BufferTooSmall,
        )?;
        Ok(())
    }
}

pub enum Op {
    Push(ProgramWord),
    Pop,
    Load(ProgramWord),
    Store(ProgramWord),
    LoadStatic,
    Jump,
    Call,
    StackLoad(ProgramWord),
    StackStore(ProgramWord),
    Dup,
    Swap,
    Return(ProgramWord),
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
    Mod,
    Add,
    Subtract,
    Exit,
}

pub struct FunctionBuilder<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    machine: MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    function_start: ProgramWord,
    index: FunctionIndex,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new(
        machine: MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        index: FunctionIndex,
    ) -> Self {
        let function_start = machine.program.free;
        Self {
            machine,
            function_start,
            index,
        }
    }

    pub fn function_start(&self) -> ProgramWord {
        self.function_start
    }

    pub fn patch_word(&mut self, index: ProgramWord, value: ProgramWord) -> Result<(), MachineBuilderError> {
        self.machine.patch_word(index, value)
    }

    pub fn add_op(&mut self, op: Op) -> Result<(), MachineBuilderError> {
        match op {
            Op::Push(value) => {
                self.machine.add_word(Ops::Push.into())?;
                self.machine.add_word(value)?;
            }
            Op::Pop => {
                self.machine.add_word(Ops::Pop.into())?;
            }
            Op::Load(address) => {
                self.machine.validate_global_address(address)?;
                self.machine.add_word(Ops::Load.into())?;
                self.machine.add_word(address)?;
            }
            Op::Store(address) => {
                self.machine.validate_global_address(address)?;
                self.machine.add_word(Ops::Store.into())?;
                self.machine.add_word(address)?;
            }
            Op::LoadStatic => {
                self.machine.add_word(Ops::LoadStatic.into())?;
            }
            Op::Jump => {
                self.machine.add_word(Ops::Jump.into())?;
            }
            Op::Call => {
                self.machine.add_word(Ops::Call.into())?;
            }
            Op::StackLoad(offset) => {
                self.machine.add_word(Ops::StackLoad.into())?;
                self.machine.add_word(offset)?;
            }
            Op::StackStore(offset) => {
                self.machine.add_word(Ops::StackStore.into())?;
                self.machine.add_word(offset)?;
            }
            Op::Dup => {
                self.machine.add_word(Ops::Dup.into())?;
            }
            Op::Swap => {
                self.machine.add_word(Ops::Swap.into())?;
            }
            Op::Return(count) => {
                self.machine.add_word(Ops::Return.into())?;
                self.machine.add_word(count)?;
            }
            Op::BranchLessThan => {
                self.machine.add_word(Ops::BranchLessThan.into())?;
            }
            Op::BranchLessThanEq => {
                self.machine.add_word(Ops::BranchLessThanEq.into())?;
            }
            Op::BranchGreaterThan => {
                self.machine.add_word(Ops::BranchGreaterThan.into())?;
            }
            Op::BranchGreaterThanEq => {
                self.machine.add_word(Ops::BranchGreaterThanEq.into())?;
            }
            Op::BranchEqual => {
                self.machine.add_word(Ops::BranchEqual.into())?;
            }
            Op::And => {
                self.machine.add_word(Ops::And.into())?;
            }
            Op::Or => {
                self.machine.add_word(Ops::Or.into())?;
            }
            Op::Xor => {
                self.machine.add_word(Ops::Xor.into())?;
            }
            Op::Not => {
                self.machine.add_word(Ops::Not.into())?;
            }
            Op::BitwiseAnd => {
                self.machine.add_word(Ops::BitwiseAnd.into())?;
            }
            Op::BitwiseOr => {
                self.machine.add_word(Ops::BitwiseOr.into())?;
            }
            Op::BitwiseXor => {
                self.machine.add_word(Ops::BitwiseXor.into())?;
            }
            Op::BitwiseNot => {
                self.machine.add_word(Ops::BitwiseNot.into())?;
            }
            Op::Multiply => {
                self.machine.add_word(Ops::Multiply.into())?;
            }
            Op::Devide => {
                self.machine.add_word(Ops::Divide.into())?;
            }
            Op::Mod => {
                self.machine.add_word(Ops::Mod.into())?;
            }
            Op::Add => {
                self.machine.add_word(Ops::Add.into())?;
            }
            Op::Subtract => {
                self.machine.add_word(Ops::Subtract.into())?;
            }
            Op::Exit => {
                self.machine.add_word(Ops::Exit.into())?;
            }
        }

        Ok(())
    }

    pub fn finish(
        mut self,
    ) -> Result<
        (
            FunctionIndex,
            MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        ),
        MachineBuilderError,
    > {
        let index = self.index.clone();
        self.machine
            .finish_function(self.function_start, self.index)?;
        Ok((index, self.machine))
    }
}

#[cfg(test)]
mod test;
