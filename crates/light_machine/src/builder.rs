use core::ops::Add;

use super::*;

#[derive(Error, Debug)]
pub enum MachineBuilderError {
    BufferTooSmall,
    MachineCountOverflowsWord(usize),
    TooLarge(usize),
    FunctionCoutExceeded,
}

pub struct Index(Word);

/// Program is
/// [machine_count][machine offsets..][machines ...]
///
pub struct ProgramBuilder<'a> {
    buffer: &'a mut [Word],
    next_builder: Word,
    free: Word,
}

impl<'a> ProgramBuilder<'a> {
    pub fn new(buffer: &'a mut [Word], machine_count: Word) -> Result<Self, MachineBuilderError> {
        // Assure `Words` can be cast to `usize`
        const { assert!(size_of::<Word>() <= size_of::<usize>()) };

        // Make sure we have at least enough space to strore
        // the offsets to each machine and the count of machines.
        // NOTE: we could probbly make some assumption about
        // the smallest usafal machine and ensure we have room
        // for that too.
        if buffer.len() <= machine_count as usize + 1 {
            return Err(MachineBuilderError::BufferTooSmall);
        }

        buffer[0] = 0;
        Ok(Self {
            buffer,
            free: machine_count + 1,
            next_builder: 1,
        })
    }

    pub fn new_machine(
        self,
        function_count: Word,
    ) -> Result<MachineBuilder<'a>, MachineBuilderError> {
        self.buffer[0] = self.next_builder;
        // SAFTY: checked in `new`
        self.buffer[self.next_builder as usize] = self.free as Word;
        MachineBuilder::new(self, function_count)
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

    fn finish_machine(&mut self) {
        self.next_builder += 1;
    }
}

/// Machine format is:
/// [function offsets...][static data + functions...]
pub struct MachineBuilder<'a> {
    program: ProgramBuilder<'a>,
    function_count: Word,
    next_function: Word,
    function_end: Word,
}

impl<'a> MachineBuilder<'a> {
    pub fn new(
        mut program: ProgramBuilder<'a>,
        function_count: Word,
    ) -> Result<Self, MachineBuilderError> {
        let next_function = program.free;
        program.allocate(function_count)?;
        let function_end = program.free;
        Ok(Self {
            program,
            function_count,
            next_function,
            function_end,
        })
    }

    pub fn add_static(&mut self, data: &[Word]) -> Result<Index, MachineBuilderError> {
        if data.len() >= Word::MAX as usize {
            return Err(MachineBuilderError::TooLarge(data.len()));
        }
        let size = data.len() as Word;
        let index = Index(self.program.free);
        self.program.allocate(size)?;
        let target = &mut self.program.buffer[index.0 as usize..(index.0 + size) as usize];
        target.copy_from_slice(data);
        Ok(index)
    }

    pub fn new_function(self) -> Result<FunctionBuilder<'a>, MachineBuilderError> {
        if self.next_function >= self.function_end {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        Ok(FunctionBuilder::new(self))
    }

    pub fn finish(mut self) -> ProgramBuilder<'a> {
        self.program.finish_machine();
        self.program
    }

    fn add_word(&mut self, word: Word) -> Result<(), MachineBuilderError> {
        self.program.add_word(word)
    }

    fn finish_function(&mut self, function_start: Word) {
        self.program.buffer[self.next_function as usize] = function_start;
        self.next_function += 1;
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
}

impl<'a> FunctionBuilder<'a> {
    pub fn new(machine: MachineBuilder<'a>) -> Self{
        let function_start = machine.program.free;
        Self {
            machine,
            function_start,
        }
    }

    pub fn add_op(&mut self, op: Op) -> Result<(), MachineBuilderError> {
        match op {
            Op::Push(address) => {
                self.machine.add_word(Ops::Push.into())?;
                self.machine.add_word(address)?;
            }
            Op::Pop => {
                self.machine.add_word(Ops::Pop.into())?;
            }
            Op::Load(address) => {
                self.machine.add_word(Ops::Load.into())?;
                self.machine.add_word(address)?;
            }
            Op::Store(address) => {
                self.machine.add_word(Ops::Store.into())?;
                self.machine.add_word(address)?;
            }
            Op::Return => {
                self.machine.add_word(Ops::Return.into())?;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> (Index, MachineBuilder<'a>) {
        let index = Index(self.function_start);
        self.machine.finish_function(self.function_start);
        (index, self.machine)
    }
}

#[cfg(test)]
mod test;
