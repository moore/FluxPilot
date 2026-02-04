use crate::{Program, ProgramNumber, Storage, StorageError, StorageErrorKind, ProgramWord};

pub struct MemStorage<'a> {
    programs: [&'a mut [ProgramWord]; 2],
    active_index: usize,
    ui_state: &'a mut [u8],
    ui_state_len: usize,
}

impl<'a> MemStorage<'a> {
    pub fn new(program: &'a mut [ProgramWord], ui_state: &'a mut [u8]) -> Self {
        // Split the provided buffer into two halves so we can swap on load completion.
        let mid = program.len() / 2;
        let (program_a, program_b) = program.split_at_mut(mid);
        Self {
            programs: [program_a, program_b],
            active_index: 0,
            ui_state,
            ui_state_len: 0,
        }
    }
}

impl<'a> Storage for MemStorage<'a> {
    type L = MemProgrameLoader;

    /// `size`` is in instrction count
    fn get_program_loader(
        &mut self,
        size: u32,
        ui_state_size: u32,
    ) -> Result<Self::L, StorageError> {
        let size: usize =
            size.try_into().map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let ui_state_size: usize = ui_state_size
            .try_into()
            .map_err(|_| StorageError::new(StorageErrorKind::UiStateTooLarge))?;
        let target_index = if self.active_index == 0 { 1 } else { 0 };
        let target = &self.programs[target_index];
        if size > target.len() {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        }
        if ui_state_size > self.ui_state.len() {
            return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
        }

        Ok(MemProgrameLoader::new(
            target_index,
            size,
            ui_state_size,
        ))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[ProgramWord],
    ) -> Result<(), StorageError> {
        loader.add_block(self.programs[loader.target_index], block_number, block)
    }

    fn add_ui_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[u8],
    ) -> Result<(), StorageError> {
        loader.add_ui_block(self.ui_state, block_number, block)
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        let target_index = loader.target_index;
        let ui_state_len = loader.finish_load()?;
        self.active_index = target_index;
        self.ui_state_len = ui_state_len;
        Ok(ProgramNumber(0))
    }

    fn get_program<'b, 'c>(
        &'b mut self,
        program_number: ProgramNumber,
        memory: &'c mut [ProgramWord],
    ) -> Result<Program<'b, 'c>, StorageError> {
        if program_number.0 != 0 {
            return Err(StorageError::new(StorageErrorKind::UnknownProgram));
        }

        match Program::new(self.programs[self.active_index], memory) {
            Ok(v) => Ok(v),
            Err(e) => Err(StorageError::invalid_program(e)),
        }
    }

    fn get_ui_state_len(&mut self, program_number: ProgramNumber) -> Result<u32, StorageError> {
        if program_number.0 != 0 {
            return Err(StorageError::new(StorageErrorKind::UnknownProgram));
        }
        Ok(self.ui_state_len as u32)
    }

    fn read_ui_state_block(
        &mut self,
        program_number: ProgramNumber,
        offset: u32,
        out: &mut [u8],
    ) -> Result<usize, StorageError> {
        if program_number.0 != 0 {
            return Err(StorageError::new(StorageErrorKind::UnknownProgram));
        }
        let offset: usize = offset
            .try_into()
            .map_err(|_| StorageError::new(StorageErrorKind::UiStateReadOutOfBounds))?;
        if offset >= self.ui_state_len {
            return Ok(0);
        }
        let remaining = self.ui_state_len - offset;
        let to_copy = remaining.min(out.len());
        let src = &self.ui_state[offset..offset + to_copy];
        out[..to_copy].copy_from_slice(src);
        Ok(to_copy)
    }
}

pub struct MemProgrameLoader {
    target_index: usize,
    program_end: usize,
    next_block: u32,
    next_word: usize,
    ui_state_len: usize,
    next_ui_block: u32,
    next_ui_offset: usize,
}

impl MemProgrameLoader {
    fn new(target_index: usize, program_end: usize, ui_state_len: usize) -> Self {
        Self {
            target_index,
            program_end,
            next_block: 0,
            next_word: 0,
            ui_state_len,
            next_ui_block: 0,
            next_ui_offset: 0,
        }
    }

    fn add_block(
        &mut self,
        program: &mut [ProgramWord],
        block_number: u32,
        block: &[ProgramWord],
    ) -> Result<(), StorageError> {
        if block_number != self.next_block {
            return Err(StorageError::new(StorageErrorKind::UnexpectedBlock));
        }

        let mut next_word = self.next_word;

        for word in block {
            if next_word >= self.program_end {
                return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
            }

            let Some(slot) = program.get_mut(next_word) else {
                return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
            };
            *slot = *word;
            let Some(updated) = next_word.checked_add(1) else {
                return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
            };
            next_word = updated;
        }

        self.next_word = next_word;
        let Some(next_block) = self.next_block.checked_add(1) else {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        };
        self.next_block = next_block;

        Ok(())
    }

    fn add_ui_block(
        &mut self,
        ui_state: &mut [u8],
        block_number: u32,
        block: &[u8],
    ) -> Result<(), StorageError> {
        if block_number != self.next_ui_block {
            return Err(StorageError::new(StorageErrorKind::UnexpectedBlock));
        }
        let mut next_offset = self.next_ui_offset;
        for &byte in block {
            if next_offset >= self.ui_state_len {
                return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
            }
            let Some(slot) = ui_state.get_mut(next_offset) else {
                return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
            };
            *slot = byte;
            let Some(updated) = next_offset.checked_add(1) else {
                return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
            };
            next_offset = updated;
        }
        self.next_ui_offset = next_offset;
        let Some(next_block) = self.next_ui_block.checked_add(1) else {
            return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
        };
        self.next_ui_block = next_block;
        Ok(())
    }

    fn finish_load(self) -> Result<usize, StorageError> {
        if self.next_word != self.program_end {
            return Err(StorageError::new(StorageErrorKind::ProgramIncomplete));
        }
        if self.next_ui_offset != self.ui_state_len {
            return Err(StorageError::new(StorageErrorKind::UiStateIncomplete));
        }
        Ok(self.ui_state_len)
    }
}
