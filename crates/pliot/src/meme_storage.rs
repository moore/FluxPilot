use crate::{Program, ProgramNumber, Storage, StorageError, Word};

pub struct MemStorage<'a> {
    programs: [&'a mut [Word]; 2],
    active_index: usize,
}

impl<'a> MemStorage<'a> {
    pub fn new(program: &'a mut [Word]) -> Self {
        // Split the provided buffer into two halves so we can swap on load completion.
        let mid = program.len() / 2;
        let (program_a, program_b) = program.split_at_mut(mid);
        Self {
            programs: [program_a, program_b],
            active_index: 0,
        }
    }
}

impl<'a> Storage for MemStorage<'a> {
    type L = MemProgrameLoader;

    /// `size`` is in instrction count
    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError> {
        let size: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;
        let target_index = if self.active_index == 0 { 1 } else { 0 };
        let target = &self.programs[target_index];
        if size > target.len() {
            return Err(StorageError::ProgramTooLarge);
        }

        Ok(MemProgrameLoader::new(target_index, size))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError> {
        loader.add_block(self.programs[loader.target_index], block_number, block)
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        let target_index = loader.target_index;
        loader.finish_load()?;
        self.active_index = target_index;
        Ok(ProgramNumber(0))
    }

    fn get_program<'b, 'c>(
        &'b mut self,
        program_number: ProgramNumber,
        globals: &'c mut [Word],
    ) -> Result<Program<'b, 'c>, StorageError> {
        if program_number.0 != 0 {
            return Err(StorageError::UnknownProgram);
        }

        match Program::new(self.programs[self.active_index], globals) {
            Ok(v) => Ok(v),
            Err(e) => Err(StorageError::InvalidProgram(e)),
        }
    }
}

pub struct MemProgrameLoader {
    target_index: usize,
    program_end: usize,
    next_block: u32,
    next_word: usize,
}

impl MemProgrameLoader {
    fn new(target_index: usize, program_end: usize) -> Self {
        Self {
            target_index,
            program_end,
            next_block: 0,
            next_word: 0,
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
        Ok(ProgramNumber(0))
    }
}
