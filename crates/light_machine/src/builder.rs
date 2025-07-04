
use super::*;

#[derive(Error, Debug)]
pub enum MachineBuilderError {
    BufferTooSmall,
    MachineCountOverflowsWord(usize),
    TooLarge(usize),
    FunctionCoutExceeded,
    GlobalOutOfRange(Word),
}

/// Index for static data.
pub struct DataIndex(Word);

/// Index for function.
pub struct FunctionIndex(Word);

/// Program is
/// [machine_count][machine offsets..][machines ...]
///
pub struct ProgramBuilder<'a> {
    buffer: &'a mut [Word],
    next_machine_builder: Word,
    free: Word,
}

impl<'a> ProgramBuilder<'a> {
    const MACHINE_COUNT_OFFSET: usize = 0;
    const GLOBALS_SIZE_OFFSET: usize = 1;
    const FUNCTIONS_OFFSET: usize = 1;


    pub fn new(buffer: &'a mut [Word], machine_count: Word) -> Result<Self, MachineBuilderError> {
        // Assure `Words` can be cast to `usize`
        const { assert!(size_of::<Word>() <= size_of::<usize>()) };

        // Make sure we have at least enough space to strore
        // the offsets to each machine and the count of machines.
        // NOTE: we could probbly make some assumption about
        // the smallest usafal machine and ensure we have room
        // for that too.
        if buffer.len() <= machine_count as usize + 2 {
            return Err(MachineBuilderError::BufferTooSmall);
        }

        buffer[Self::MACHINE_COUNT_OFFSET] = 0; // Machine count
        buffer[Self::GLOBALS_SIZE_OFFSET] = 0; // Globals size
        Ok(Self {
            buffer,
            free: machine_count + 2,
            next_machine_builder: 1,
        })
    }

    pub fn new_machine(
        self,
        function_count: Word,
        globals_size: Word,
    ) -> Result<MachineBuilder<'a>, MachineBuilderError> {
        self.buffer[Self::MACHINE_COUNT_OFFSET] = self.next_machine_builder;
        // SAFTY: checked in `new`
        self.buffer[self.next_machine_builder as usize + Self::FUNCTIONS_OFFSET] = self.free as Word;
        let globals_offset = self.buffer[Self::GLOBALS_SIZE_OFFSET];
        MachineBuilder::new(self, function_count, globals_size, globals_offset)
    }

    fn allocate(&mut self, word_count: Word) -> Result<(), MachineBuilderError> {
        if self.free as usize + word_count as usize > self.buffer.len() {
            return Err(MachineBuilderError::BufferTooSmall);
        }
        self.free += word_count;
        Ok(())
    }

    fn add_word(&mut self, word: Word) -> Result<(), MachineBuilderError> {
        if self.free as usize >= self.buffer.len() {
            return Err(MachineBuilderError::BufferTooSmall);
        }
        self.buffer[self.free as usize] = word;
        self.free += 1;

        return Ok(())
    }

    fn finish_machine(&mut self, globals_size: Word) {
        self.buffer[Self::GLOBALS_SIZE_OFFSET] += globals_size;
        self.next_machine_builder += 1;
    }
}

/// Machine format is:
/// [function offsets...][static data + functions...]
pub struct MachineBuilder<'a> {
    program: ProgramBuilder<'a>,
    function_count: Word,
    next_function: Word,
    function_end: Word,
    globals_size: Word,
}

impl<'a> MachineBuilder<'a> {
    pub fn new(
        mut program: ProgramBuilder<'a>,
        function_count: Word,
        globals_size: Word,
        globals_offset: Word,
    ) -> Result<Self, MachineBuilderError> {
        program.add_word(globals_size)?;
        program.add_word(globals_offset)?;
        let next_function = program.free;
        program.allocate(function_count)?;
        let function_end = program.free;
        Ok(Self {
            program,
            function_count,
            next_function,
            function_end,
            globals_size,
        })
    }

    pub fn add_static(&mut self, data: &[Word]) -> Result<DataIndex, MachineBuilderError> {
        if data.len() >= Word::MAX as usize {
            return Err(MachineBuilderError::TooLarge(data.len()));
        }
        let size = data.len() as Word;
        let index = DataIndex(self.program.free);
        self.program.allocate(size)?;
        let target = &mut self.program.buffer[index.0 as usize..(index.0 + size) as usize];
        target.copy_from_slice(data);
        Ok(index)
    }

    pub fn reserve_function(&mut self) -> Result<FunctionIndex, MachineBuilderError> {
        if self.next_function >= self.function_end {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        let index = FunctionIndex(self.next_function);
        self.next_function += 1;
        Ok(index)
    }

    pub fn new_function(mut self) -> Result<FunctionBuilder<'a>, MachineBuilderError> {
        let index = self.reserve_function()?;
        Ok(FunctionBuilder::new(self, index))
    }

    pub fn new_function_at_index(self, index: FunctionIndex) -> Result<FunctionBuilder<'a>, MachineBuilderError> {
        Ok(FunctionBuilder::new(self, index))
    }

    pub fn finish(mut self) -> ProgramBuilder<'a> {
        self.program.finish_machine(self.globals_size);
        self.program
    }

    fn add_word(&mut self, word: Word) -> Result<(), MachineBuilderError> {
        self.program.add_word(word)
    }

    fn finish_function(&mut self, function_start: Word, index: FunctionIndex) {
        self.program.buffer[index.0 as usize] = function_start;
    }
}


pub enum Op {
    Push(Word),
    Pop,
    Load(Word),
    Store(Word),
    Return,
}

pub struct FunctionBuilder<'a> {
    machine: MachineBuilder<'a>,
    function_start: Word,
    index: FunctionIndex,
}

impl<'a> FunctionBuilder<'a> {
    pub fn new(machine: MachineBuilder<'a>, index: FunctionIndex) -> Self{
        let function_start = machine.program.free;
        Self {
            machine,
            function_start,
            index,
        }
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
                if address >= self.machine.globals_size {
                    return Err(MachineBuilderError::GlobalOutOfRange(address));
                }
                self.machine.add_word(Ops::Load.into())?;
                self.machine.add_word(address)?;
            }
            Op::Store(address) => {
                if address >= self.machine.globals_size {
                    return Err(MachineBuilderError::GlobalOutOfRange(address));
                }
                self.machine.add_word(Ops::Store.into())?;
                self.machine.add_word(address)?;
            }
            Op::Return => {
                self.machine.add_word(Ops::Return.into())?;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> (FunctionIndex, MachineBuilder<'a>) {
        let index = FunctionIndex(self.function_start);
        self.machine.finish_function(self.function_start, self.index);
        (index, self.machine)
    }
}

#[cfg(test)]
mod test;
