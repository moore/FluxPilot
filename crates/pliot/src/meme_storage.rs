use crate::{Program, ProgramNumber, Storage, StorageError, Word};

pub struct MemStorage<'a> {
    program: &'a mut [Word],
    next_program: usize,
}

impl<'a> MemStorage<'a> {
    pub fn new(program: &'a mut [Word]) -> Self {
        Self {
            program,
            next_program: 0,
        }
    }
}

impl<'a> Storage for MemStorage<'a> {
    type L = MemProgrameLoader;

    /// `size`` is in instrction count
    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError> {
        let size: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;

        let Some(available) = self.program.len().checked_sub(self.next_program) else {
            return Err(StorageError::ProgramTooLarge);
        };
        if size > available {
            return Err(StorageError::ProgramTooLarge);
        }

        let program_start = self.next_program;
        let Some(next_program) = self.next_program.checked_add(size) else {
            return Err(StorageError::ProgramTooLarge);
        };
        self.next_program = next_program;

        Ok(MemProgrameLoader::new(program_start, self.next_program))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError> {
        loader.add_block(self.program, block_number, block)
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        loader.finish_load()
    }

    fn get_program<'b, 'c>(
        &'b mut self,
        program_number: ProgramNumber,
        globals: &'c mut [Word],
    ) -> Result<Program<'b, 'c>, StorageError> {
        if program_number.0 != 0 {
            return Err(StorageError::UnknownProgram);
        }

        match Program::new(self.program, globals) {
            Ok(v) => Ok(v),
            Err(e) => Err(StorageError::InvalidProgram(e)),
        }
    }
}

pub struct MemProgrameLoader {
    program_start: usize,
    program_end: usize,
    next_block: u32,
    next_word: usize,
}

impl MemProgrameLoader {
    fn new(program_start: usize, program_end: usize) -> Self {
        Self {
            program_start,
            program_end,
            next_block: 0,
            next_word: program_start,
        }
    }

    fn add_block(
        &mut self,
        program: &mut [Word],
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError> {
        if block_number != self.next_block {
            return Err(StorageError::UnexpectedBlock);
        }

        let mut next_word = self.next_word;

        for word in block {
            if next_word >= self.program_end {
                return Err(StorageError::ProgramTooLarge);
            }

            let Some(slot) = program.get_mut(next_word) else {
                return Err(StorageError::ProgramTooLarge);
            };
            *slot = *word;
            let Some(updated) = next_word.checked_add(1) else {
                return Err(StorageError::ProgramTooLarge);
            };
            next_word = updated;
        }

        self.next_word = next_word;
        let Some(next_block) = self.next_block.checked_add(1) else {
            return Err(StorageError::ProgramTooLarge);
        };
        self.next_block = next_block;

        Ok(())
    }

    fn finish_load(self) -> Result<ProgramNumber, StorageError> {
        Ok(ProgramNumber(self.program_start))
    }
}
