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

impl DataIndex {
    pub fn to_word(&self) -> ProgramWord {
        self.0
    }
}

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
    instance_count: ProgramWord,
    type_count: ProgramWord,
    next_type_builder: ProgramWord,
    next_instance_number: ProgramWord,
    free: ProgramWord,
    descriptor: ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    shared_globals_size: ProgramWord,
    shared_function_count: ProgramWord,
    next_shared_function_number: ProgramWord,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new(
        buffer: &'a mut [ProgramWord],
        instance_count: ProgramWord,
        type_count: ProgramWord,
        shared_function_count: ProgramWord,
    ) -> Result<Self, MachineBuilderError> {
        // Assure `Words` can be cast to `usize`
        const { assert!(size_of::<ProgramWord>() <= size_of::<usize>()) };

        // Make sure we have at least enough space to strore
        // the offsets to each machine and the count of machines.
        // NOTE: we could probbly make some assumption about
        // the smallest usafal machine and ensure we have room
        // for that too.
        let instance_table_words = (instance_count as usize)
            .checked_mul(2)
            .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                instance_count as usize,
            ))?;
        let type_table_words = (type_count as usize)
            .checked_mul(2)
            .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                type_count as usize,
            ))?;
        let Some(required) = HEADER_WORDS
            .checked_add(instance_table_words)
            .and_then(|count| count.checked_add(type_table_words))
            .and_then(|count| count.checked_add(shared_function_count as usize)) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                instance_count as usize,
            ));
        };
        if buffer.len() <= required {
            return Err(MachineBuilderError::BufferTooSmall);
        }

        set_value(buffer, VERSION_OFFSET, PROGRAM_VERSION, MachineBuilderError::BufferTooSmall)?;
        set_value(
            buffer,
            MACHINE_COUNT_OFFSET,
            instance_count,
            MachineBuilderError::BufferTooSmall,
        )?;
        set_value(buffer, GLOBALS_SIZE_OFFSET, 0, MachineBuilderError::BufferTooSmall)?; // Globals size
        set_value(
            buffer,
            SHARED_FUNCTION_COUNT_OFFSET,
            shared_function_count,
            MachineBuilderError::BufferTooSmall,
        )?;
        set_value(
            buffer,
            TYPE_COUNT_OFFSET,
            type_count,
            MachineBuilderError::BufferTooSmall,
        )?;
        let instance_table_offset = ProgramWord::try_from(HEADER_WORDS)
            .map_err(|_| MachineBuilderError::MachineCountOverflowsWord(HEADER_WORDS))?;
        set_value(
            buffer,
            INSTANCE_TABLE_OFFSET,
            instance_table_offset,
            MachineBuilderError::BufferTooSmall,
        )?;
        let instance_table_words = ProgramWord::try_from(instance_table_words)
            .map_err(|_| MachineBuilderError::MachineCountOverflowsWord(instance_table_words))?;
        let type_table_offset = instance_table_offset
            .checked_add(instance_table_words)
            .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                instance_count as usize,
            ))?;
        set_value(
            buffer,
            TYPE_TABLE_OFFSET,
            type_table_offset,
            MachineBuilderError::BufferTooSmall,
        )?;
        let type_table_words = ProgramWord::try_from(type_table_words)
            .map_err(|_| MachineBuilderError::MachineCountOverflowsWord(type_table_words))?;
        let shared_function_table_offset = type_table_offset
            .checked_add(type_table_words)
            .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                type_count as usize,
            ))?;
        set_value(
            buffer,
            SHARED_FUNCTION_TABLE_OFFSET,
            shared_function_table_offset,
            MachineBuilderError::BufferTooSmall,
        )?;
        let free = shared_function_table_offset
            .checked_add(shared_function_count)
            .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                shared_function_count as usize,
            ))?;
        Ok(Self {
            buffer,
            instance_count,
            type_count,
            free,
            next_type_builder: 0,
            next_instance_number: 0,
            descriptor: ProgramDescriptor::new(),
            shared_globals_size: 0,
            shared_function_count,
            next_shared_function_number: 0,
        })
    }

    pub fn shared_function_count(&self) -> ProgramWord {
        self.shared_function_count
    }

    pub fn program_free(&self) -> ProgramWord {
        self.free
    }

    pub fn set_shared_globals_size(&mut self, shared_globals_size: ProgramWord) -> Result<(), MachineBuilderError> {
        if self.next_type_builder != 0 || self.next_instance_number != 0 || self.next_shared_function_number != 0 {
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
        if self.next_type_builder >= self.type_count {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        if self.next_instance_number >= self.instance_count {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        let Some(_next_type_builder) = self.next_type_builder.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_type_builder as usize,
            ));
        };
        let Some(_next_instance_number) = self.next_instance_number.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_instance_number as usize,
            ));
        };
        let instance_table_offset = read_static(INSTANCE_TABLE_OFFSET, self.buffer)
            .map_err(|_| MachineBuilderError::BufferTooSmall)? as usize;
        let instance_entry_index = (self.next_instance_number as usize)
            .checked_mul(2)
            .and_then(|offset| instance_table_offset.checked_add(offset))
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        let globals_base = *self
            .buffer
            .get(GLOBALS_SIZE_OFFSET)
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        let type_id = self.next_type_builder;
        set_value(
            self.buffer,
            instance_entry_index,
            type_id,
            MachineBuilderError::BufferTooSmall,
        )?;
        set_value(
            self.buffer,
            instance_entry_index
                .checked_add(1)
                .ok_or(MachineBuilderError::BufferTooSmall)?,
            globals_base,
            MachineBuilderError::BufferTooSmall,
        )?;
        let type_table_offset = read_static(TYPE_TABLE_OFFSET, self.buffer)
            .map_err(|_| MachineBuilderError::BufferTooSmall)? as usize;
        let type_entry_index = (type_id as usize)
            .checked_mul(2)
            .and_then(|offset| type_table_offset.checked_add(offset))
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        set_value(
            self.buffer,
            type_entry_index,
            function_count,
            MachineBuilderError::BufferTooSmall,
        )?;
        set_value(
            self.buffer,
            type_entry_index
                .checked_add(1)
                .ok_or(MachineBuilderError::BufferTooSmall)?,
            self.free,
            MachineBuilderError::BufferTooSmall,
        )?;
        let shared_globals_size = self.shared_globals_size;
        MachineBuilder::new(
            self,
            function_count,
            globals_size,
            globals_base,
            shared_globals_size,
            type_id,
        )
    }

    pub fn add_instance(
        &mut self,
        type_id: ProgramWord,
    ) -> Result<(), MachineBuilderError> {
        if self.next_instance_number >= self.instance_count {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        let type_descriptor = self
            .descriptor
            .types
            .get(type_id as usize)
            .ok_or(MachineBuilderError::MachineCountExceeded)?;
        let instance_table_offset = read_static(INSTANCE_TABLE_OFFSET, self.buffer)
            .map_err(|_| MachineBuilderError::BufferTooSmall)? as usize;
        let instance_entry_index = (self.next_instance_number as usize)
            .checked_mul(2)
            .and_then(|offset| instance_table_offset.checked_add(offset))
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        let globals_base = *self
            .buffer
            .get(GLOBALS_SIZE_OFFSET)
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        set_value(
            self.buffer,
            instance_entry_index,
            type_id,
            MachineBuilderError::BufferTooSmall,
        )?;
        set_value(
            self.buffer,
            instance_entry_index
                .checked_add(1)
                .ok_or(MachineBuilderError::BufferTooSmall)?,
            globals_base,
            MachineBuilderError::BufferTooSmall,
        )?;
        let globals_slot = get_mut_or(
            self.buffer,
            GLOBALS_SIZE_OFFSET,
            MachineBuilderError::BufferTooSmall,
        )?;
        let Some(new_globals_size) = globals_slot.checked_add(type_descriptor.globals_size) else {
            return Err(MachineBuilderError::TooLarge(type_descriptor.globals_size as usize));
        };
        *globals_slot = new_globals_size;
        let Some(next_instance_number) = self.next_instance_number.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_instance_number as usize,
            ));
        };
        self.next_instance_number = next_instance_number;
        if self
            .descriptor
            .add_instance(MachineInstanceDescriptor {
                type_id,
                globals_base,
            })
            .is_err()
        {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        Ok(())
    }

    pub fn new_shared_function(
        mut self,
    ) -> Result<
        SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        MachineBuilderError,
    > {
        let index = self.reserve_shared_function()?;
        Ok(SharedFunctionBuilder::new(self, index))
    }

    pub fn new_shared_function_at_index(
        self,
        index: FunctionIndex,
    ) -> Result<
        SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        MachineBuilderError,
    > {
        if index.0 >= self.shared_function_count {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        Ok(SharedFunctionBuilder::new(self, index))
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

    fn finish_machine(
        &mut self,
        globals_size: ProgramWord,
        machine_descriptor: MachineTypeDescriptor<FUNCTION_COUNT_MAX>,
        type_id: ProgramWord,
        globals_base: ProgramWord,
    ) -> Result<(), MachineBuilderError>{
        if self.descriptor.add_type(machine_descriptor).is_err() {
            return Err(MachineBuilderError::MachineCountExceeded);
        }
        if self
            .descriptor
            .add_instance(MachineInstanceDescriptor {
                type_id,
                globals_base,
            })
            .is_err()
        {
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
        let Some(next_type_builder) = self.next_type_builder.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_type_builder as usize,
            ));
        };
        self.next_type_builder = next_type_builder;
        let Some(next_instance_number) = self.next_instance_number.checked_add(1) else {
            return Err(MachineBuilderError::MachineCountOverflowsWord(
                self.next_instance_number as usize,
            ));
        };
        self.next_instance_number = next_instance_number;
        Ok(())
    }

    fn reserve_shared_function(&mut self) -> Result<FunctionIndex, MachineBuilderError> {
        if self.next_shared_function_number >= self.shared_function_count {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        let index = FunctionIndex(self.next_shared_function_number);
        let Some(next_shared_function_number) =
            self.next_shared_function_number.checked_add(1)
        else {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        };
        self.next_shared_function_number = next_shared_function_number;
        Ok(index)
    }

    fn finish_shared_function(
        &mut self,
        function_start: ProgramWord,
        index: FunctionIndex,
    ) -> Result<(), MachineBuilderError> {
        let table_base = read_static(SHARED_FUNCTION_TABLE_OFFSET, self.buffer)
            .map_err(|_| MachineBuilderError::BufferTooSmall)? as usize;
        let entry_index = table_base
            .checked_add(usize::from(index.0))
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        set_value(
            self.buffer,
            entry_index,
            function_start,
            MachineBuilderError::BufferTooSmall,
        )?;
        Ok(())
    }

    pub fn add_shared_static(&mut self, data: &[ProgramWord]) -> Result<DataIndex, MachineBuilderError> {
        if data.len() >= ProgramWord::MAX as usize {
            return Err(MachineBuilderError::TooLarge(data.len()));
        }
        let size = data.len() as ProgramWord;
        let index = DataIndex(self.free);
        self.allocate(size)?;
        let start = usize::from(index.0);
        let end = start
            .checked_add(usize::from(size))
            .ok_or(MachineBuilderError::BufferTooSmall)?;
        let Some(target) = self.buffer.get_mut(start..end) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        target.copy_from_slice(data);
        Ok(index)
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
    function_table_start: ProgramWord,
    function_count: ProgramWord,
    next_function_number: ProgramWord,
    globals_size: ProgramWord,
    globals_offset: ProgramWord,
    shared_globals_size: ProgramWord,
    type_id: ProgramWord,
    discriptor: MachineTypeDescriptor<FUNCTION_COUNT_MAX>,
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
        type_id: ProgramWord,
    ) -> Result<Self, MachineBuilderError> {
        let function_table_start = program.free;
        program.allocate(function_count)?;
        Ok(Self {
            program,
            function_table_start,
            function_count,
            next_function_number: 0,
            globals_size,
            globals_offset,
            shared_globals_size,
            type_id,
            discriptor: MachineTypeDescriptor::new(globals_size),
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
        let globals_base = self.globals_offset;
        let type_id = self.type_id;
        self.program
            .finish_machine(self.globals_size, self.discriptor, type_id, globals_base)?;
        Ok(self.program)
    }

    pub fn globals_offset(&self) -> ProgramWord {
        self.globals_offset
    }

    fn validate_local_offset(&self, offset: ProgramWord) -> Result<(), MachineBuilderError> {
        if offset >= self.globals_size {
            return Err(MachineBuilderError::GlobalOutOfRange(offset));
        }
        Ok(())
    }

    fn validate_shared_global_address(&self, address: ProgramWord) -> Result<(), MachineBuilderError> {
        if address < self.shared_globals_size {
            return Ok(());
        }
        Err(MachineBuilderError::GlobalOutOfRange(address))
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
        let base = usize::from(self.function_table_start);
        let Some(index) = base.checked_add(usize::from(index.0)) else {
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
    LocalLoad(ProgramWord),
    LocalStore(ProgramWord),
    GlobalLoad(ProgramWord),
    GlobalStore(ProgramWord),
    LoadStatic,
    Jump,
    Call,
    CallShared,
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
            Op::LocalLoad(offset) => {
                self.machine.validate_local_offset(offset)?;
                self.machine.add_word(Ops::LocalLoad.into())?;
                self.machine.add_word(offset)?;
            }
            Op::LocalStore(offset) => {
                self.machine.validate_local_offset(offset)?;
                self.machine.add_word(Ops::LocalStore.into())?;
                self.machine.add_word(offset)?;
            }
            Op::GlobalLoad(address) => {
                self.machine.validate_shared_global_address(address)?;
                self.machine.add_word(Ops::GlobalLoad.into())?;
                self.machine.add_word(address)?;
            }
            Op::GlobalStore(address) => {
                self.machine.validate_shared_global_address(address)?;
                self.machine.add_word(Ops::GlobalStore.into())?;
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
            Op::CallShared => {
                self.machine.add_word(Ops::CallShared.into())?;
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

    pub fn add_raw_word(&mut self, word: ProgramWord) -> Result<(), MachineBuilderError> {
        self.machine.add_word(word)
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

pub struct SharedFunctionBuilder<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize> {
    program: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    function_start: ProgramWord,
    index: FunctionIndex,
    shared_globals_size: ProgramWord,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>
    SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>
{
    pub fn new(
        program: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        index: FunctionIndex,
    ) -> Self {
        let function_start = program.free;
        let shared_globals_size = program.shared_globals_size;
        Self {
            program,
            function_start,
            index,
            shared_globals_size,
        }
    }

    pub fn function_start(&self) -> ProgramWord {
        self.function_start
    }

    pub fn patch_word(&mut self, index: ProgramWord, value: ProgramWord) -> Result<(), MachineBuilderError> {
        let index = usize::from(index);
        let Some(slot) = self.program.buffer.get_mut(index) else {
            return Err(MachineBuilderError::BufferTooSmall);
        };
        *slot = value;
        Ok(())
    }

    fn validate_shared_global_address(&self, address: ProgramWord) -> Result<(), MachineBuilderError> {
        if address < self.shared_globals_size {
            return Ok(());
        }
        Err(MachineBuilderError::GlobalOutOfRange(address))
    }

    pub fn add_op(&mut self, op: Op) -> Result<(), MachineBuilderError> {
        match op {
            Op::Push(value) => {
                self.program.add_word(Ops::Push.into())?;
                self.program.add_word(value)?;
            }
            Op::Pop => {
                self.program.add_word(Ops::Pop.into())?;
            }
            Op::LocalLoad(offset) => {
                self.program.add_word(Ops::LocalLoad.into())?;
                self.program.add_word(offset)?;
            }
            Op::LocalStore(offset) => {
                self.program.add_word(Ops::LocalStore.into())?;
                self.program.add_word(offset)?;
            }
            Op::GlobalLoad(address) => {
                self.validate_shared_global_address(address)?;
                self.program.add_word(Ops::GlobalLoad.into())?;
                self.program.add_word(address)?;
            }
            Op::GlobalStore(address) => {
                self.validate_shared_global_address(address)?;
                self.program.add_word(Ops::GlobalStore.into())?;
                self.program.add_word(address)?;
            }
            Op::LoadStatic => {
                self.program.add_word(Ops::LoadStatic.into())?;
            }
            Op::Jump => {
                self.program.add_word(Ops::Jump.into())?;
            }
            Op::Call => {
                self.program.add_word(Ops::Call.into())?;
            }
            Op::CallShared => {
                self.program.add_word(Ops::CallShared.into())?;
            }
            Op::StackLoad(offset) => {
                self.program.add_word(Ops::StackLoad.into())?;
                self.program.add_word(offset)?;
            }
            Op::StackStore(offset) => {
                self.program.add_word(Ops::StackStore.into())?;
                self.program.add_word(offset)?;
            }
            Op::Dup => {
                self.program.add_word(Ops::Dup.into())?;
            }
            Op::Swap => {
                self.program.add_word(Ops::Swap.into())?;
            }
            Op::Return(count) => {
                self.program.add_word(Ops::Return.into())?;
                self.program.add_word(count)?;
            }
            Op::BranchLessThan => {
                self.program.add_word(Ops::BranchLessThan.into())?;
            }
            Op::BranchLessThanEq => {
                self.program.add_word(Ops::BranchLessThanEq.into())?;
            }
            Op::BranchGreaterThan => {
                self.program.add_word(Ops::BranchGreaterThan.into())?;
            }
            Op::BranchGreaterThanEq => {
                self.program.add_word(Ops::BranchGreaterThanEq.into())?;
            }
            Op::BranchEqual => {
                self.program.add_word(Ops::BranchEqual.into())?;
            }
            Op::And => {
                self.program.add_word(Ops::And.into())?;
            }
            Op::Or => {
                self.program.add_word(Ops::Or.into())?;
            }
            Op::Xor => {
                self.program.add_word(Ops::Xor.into())?;
            }
            Op::Not => {
                self.program.add_word(Ops::Not.into())?;
            }
            Op::BitwiseAnd => {
                self.program.add_word(Ops::BitwiseAnd.into())?;
            }
            Op::BitwiseOr => {
                self.program.add_word(Ops::BitwiseOr.into())?;
            }
            Op::BitwiseXor => {
                self.program.add_word(Ops::BitwiseXor.into())?;
            }
            Op::BitwiseNot => {
                self.program.add_word(Ops::BitwiseNot.into())?;
            }
            Op::Multiply => {
                self.program.add_word(Ops::Multiply.into())?;
            }
            Op::Devide => {
                self.program.add_word(Ops::Divide.into())?;
            }
            Op::Mod => {
                self.program.add_word(Ops::Mod.into())?;
            }
            Op::Add => {
                self.program.add_word(Ops::Add.into())?;
            }
            Op::Subtract => {
                self.program.add_word(Ops::Subtract.into())?;
            }
            Op::Exit => {
                self.program.add_word(Ops::Exit.into())?;
            }
        }
        Ok(())
    }

    pub fn add_raw_word(&mut self, word: ProgramWord) -> Result<(), MachineBuilderError> {
        self.program.add_word(word)
    }

    pub fn finish(
        mut self,
    ) -> Result<
        (
            FunctionIndex,
            ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
        ),
        MachineBuilderError,
    > {
        let index = self.index.clone();
        self.program
            .finish_shared_function(self.function_start, self.index)?;
        Ok((index, self.program))
    }
}

#[cfg(test)]
mod test;
