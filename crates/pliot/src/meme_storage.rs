use crate::{Program, ProgramLoader, ProgramNumber, Storage, StorageError, Word};

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
    type L<'c>
        = MemProgrameLoader<'c>
    where
        Self: 'c;

    /// `size`` is in instrction count
    fn get_program_loader<'b>(&'b mut self, size: u32) -> Result<Self::L<'b>, StorageError> {
        let size: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;

        if size > self.program.len() - self.next_program {
            return Err(StorageError::ProgramTooLarge);
        }

        let program_start = self.next_program;
        self.next_program += size;

        Ok(MemProgrameLoader::new(
            self.program,
            program_start,
            self.next_program,
        ))
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

pub struct MemProgrameLoader<'a> {
    program: &'a mut [Word],
    program_start: usize,
    program_end: usize,
    next_block: u32,
    next_word: usize,
}

impl<'a> MemProgrameLoader<'a> {
    fn new(program: &'a mut [Word], program_start: usize, program_end: usize) -> Self {
        return Self {
            program,
            program_start,
            program_end,
            next_block: 0,
            next_word: program_start,
        };
    }
}

impl<'a> ProgramLoader<'a> for MemProgrameLoader<'a> {
    fn add_block(&mut self, block_number: u32, block: &[Word]) -> Result<(), StorageError> {
        if block_number != self.next_block {
            return Err(StorageError::UnexpectedBlock);
        }

        let mut next_word = self.next_word;

        for word in block {
            if next_word >= self.program_end {
                return Err(StorageError::ProgramTooLarge);
            }

            self.program[next_word] = *word;
            next_word += 1;
        }

        self.next_word = next_word;
        self.next_block += 1;

        Ok(())
    }

    fn finish_load(self) -> Result<ProgramNumber, StorageError> {
        Ok(ProgramNumber(self.program_start))
    }
}
