//! Flash-backed storage for Pliot programs using embedded-storage flash traits.
use embedded_storage::nor_flash::{NorFlash, NorFlashError, NorFlashErrorKind};
use light_machine::{Program, ProgramWord, StackWord};
use pliot::{ProgramNumber, Storage, StorageError, StorageErrorKind};

const WORD_SIZE_BYTES: usize = 2;
// Header layout: magic, version, program length (words), program crc32,
// ui state length (bytes), ui state crc32, sequence, header crc32.
const HEADER_MAGIC: u32 = u32::from_le_bytes(*b"PLIO");
const HEADER_VERSION: u32 = 3;
const HEADER_SIZE_BYTES: usize = 32;
const HEADER_WORDS: usize = HEADER_SIZE_BYTES / WORD_SIZE_BYTES;
const SLOT_COUNT: usize = 2;
const MAX_WRITE_BUFFER: usize = 32;

extern "C" {
    static __storage_start: u8;
    static __storage_end: u8;
}

pub struct FlashStorage<F: NorFlash> {
    flash: F,
    storage_start: usize,
    storage_end: usize,
    storage_offset: u32,
    program_words: usize,
    ui_state_len: usize,
    active_slot: usize,
    active_sequence: u32,
    slot_len: usize,
}

impl<F: NorFlash> FlashStorage<F> {
    pub fn new(flash: F, flash_base: usize) -> Result<Self, StorageError> {
        let (storage_start, storage_end) = storage_bounds();
        Self::new_with_bounds(flash, flash_base, storage_start, storage_end)
    }

    pub fn new_with_bounds(
        flash: F,
        flash_base: usize,
        storage_start: usize,
        storage_end: usize,
    ) -> Result<Self, StorageError> {
        let storage_len = storage_end
            .checked_sub(storage_start)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        if storage_len < HEADER_SIZE_BYTES {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let storage_offset = storage_start
            .checked_sub(flash_base)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let storage_offset = u32::try_from(storage_offset).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
        if storage_offset as usize % F::READ_SIZE != 0
            || storage_offset as usize % F::WRITE_SIZE != 0
            || storage_offset as usize % F::ERASE_SIZE != 0
        {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        if WORD_SIZE_BYTES % F::WRITE_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        if HEADER_SIZE_BYTES % F::WRITE_SIZE != 0 || HEADER_SIZE_BYTES % F::READ_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        let storage_end_offset = storage_offset
            .checked_add(
                u32::try_from(storage_len).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        if storage_end_offset as usize > flash.capacity() {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let slot_len = storage_len
            .checked_div(SLOT_COUNT)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let slot_len = slot_len - (slot_len % F::ERASE_SIZE);
        if slot_len < HEADER_SIZE_BYTES {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let total_slots_len = slot_len
            .checked_mul(SLOT_COUNT)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        if total_slots_len > storage_len {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        Ok(Self {
            flash,
            storage_start,
            storage_end,
            storage_offset,
            program_words: 0,
            ui_state_len: 0,
            active_slot: 0,
            active_sequence: 0,
            slot_len,
        })
    }

    pub fn load_header(&mut self) -> Result<(), StorageError> {
        let mut best: Option<(usize, StorageHeader)> = None;
        for slot in 0..SLOT_COUNT {
            let Ok(header) = self.read_header(slot) else {
                continue;
            };
            let Ok(program_words) = usize::try_from(header.program_words) else {
                continue;
            };
            if !self.validate_program_crc(slot, program_words, header.program_crc) {
                continue;
            }
            let Ok(ui_state_len) = usize::try_from(header.ui_state_len) else {
                continue;
            };
            if !self.validate_ui_state_crc(slot, program_words, ui_state_len, header.ui_state_crc)
            {
                continue;
            }
            match best {
                None => best = Some((slot, header)),
                Some((_, ref current)) => {
                    if is_seq_newer(header.sequence, current.sequence) {
                        best = Some((slot, header));
                    }
                }
            }
        }

        if let Some((slot, header)) = best {
            let program_words =
                usize::try_from(header.program_words).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
            let ui_state_len =
                usize::try_from(header.ui_state_len).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
            self.program_words = program_words;
            self.ui_state_len = ui_state_len;
            self.active_slot = slot;
            self.active_sequence = header.sequence;
        } else {
            self.program_words = 0;
            self.ui_state_len = 0;
            self.active_slot = 0;
            self.active_sequence = 0;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.program_words == 0
    }

    pub fn probe_write_read(&mut self) -> Result<(), StorageError> {
        const TEST_VALUE: ProgramWord = 0xA5A5;
        let offset = self.storage_offset;
        let erase_len = align_up(WORD_SIZE_BYTES, F::ERASE_SIZE)?;
        self.flash_erase_range(offset, erase_len)?;

        let erased = self.read_word(offset)?;
        if erased != 0xFFFF {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }

        self.flash_program_words(offset, &[TEST_VALUE])?;
        let read_value = self.read_word(offset)?;
        if read_value != TEST_VALUE {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        Ok(())
    }

    pub fn format(&mut self) -> Result<(), StorageError> {
        let erase_len = align_up(self.slot_len, F::ERASE_SIZE)?;
        for slot in 0..SLOT_COUNT {
            let slot_offset = self.slot_offset(slot)?;
            self.flash_erase_range(slot_offset, erase_len)?;
            self.flash_program_header(slot_offset, 0, crc32_empty(), 0, crc32_empty(), 0)?;
        }
        self.load_header()
    }

    pub fn write_program(&mut self, program: &[ProgramWord]) -> Result<(), StorageError> {
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let target_slot = self.inactive_slot();
        let capacity = self
            .slot_len
            .checked_sub(HEADER_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        if byte_len > capacity {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        }

        let total_len = byte_len
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let erase_len = align_up(total_len, F::ERASE_SIZE)?;

        let slot_offset = self.slot_offset(target_slot)?;
        self.flash_erase_range(slot_offset, erase_len)?;
        let program_offset = slot_offset
            .checked_add(HEADER_SIZE_BYTES as u32)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        self.flash_program_words(program_offset, program)?;
        let program_crc = crc32_words(program);
        let sequence = self.active_sequence.wrapping_add(1);
        self.flash_program_header(
            slot_offset,
            program.len(),
            program_crc,
            0,
            crc32_empty(),
            sequence,
        )?;

        self.program_words = program.len();
        self.ui_state_len = 0;
        self.active_slot = target_slot;
        self.active_sequence = sequence;
        Ok(())
    }

    fn slot_offset(&self, slot: usize) -> Result<u32, StorageError> {
        if slot >= SLOT_COUNT {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let slot_offset = self
            .slot_len
            .checked_mul(slot)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let slot_offset =
            u32::try_from(slot_offset).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
        self.storage_offset
            .checked_add(slot_offset)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))
    }

    fn slot_start_addr(&self, slot: usize) -> Result<usize, StorageError> {
        if slot >= SLOT_COUNT {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let slot_offset = self
            .slot_len
            .checked_mul(slot)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        self.storage_start
            .checked_add(slot_offset)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))
    }

    fn slot_end_addr(&self, slot: usize) -> Result<usize, StorageError> {
        let slot_start = self.slot_start_addr(slot)?;
        slot_start
            .checked_add(self.slot_len)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))
    }

    fn program_start_addr(&self, slot: usize) -> Result<usize, StorageError> {
        let slot_start = self.slot_start_addr(slot)?;
        slot_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))
    }

    fn inactive_slot(&self) -> usize {
        if self.active_slot == 0 { 1 } else { 0 }
    }

    fn slice_program_words<'a>(
        &'a self,
        start: usize,
        program_words: usize,
    ) -> Option<&'a [ProgramWord]> {
        let bytes_len = program_words.checked_mul(WORD_SIZE_BYTES)?;
        let end = start.checked_add(bytes_len)?;
        if start < self.storage_start || end > self.storage_end {
            return None;
        }
        if !start.is_multiple_of(core::mem::align_of::<ProgramWord>()) {
            return None;
        }
        // SAFETY: Bounds and alignment are checked against the linker-defined storage range.
        Some(unsafe { core::slice::from_raw_parts(start as *const ProgramWord, program_words) })
    }

    fn slice_bytes<'a>(&'a self, start: usize, len: usize) -> Option<&'a [u8]> {
        let end = start.checked_add(len)?;
        if start < self.storage_start || end > self.storage_end {
            return None;
        }
        // SAFETY: Bounds are checked against the linker-defined storage range.
        Some(unsafe { core::slice::from_raw_parts(start as *const u8, len) })
    }

    fn program_slice_for_slot<'a>(
        &'a self,
        slot: usize,
        program_words: usize,
    ) -> Option<&'a [ProgramWord]> {
        let (start, end) = self.program_bounds(slot)?;
        let max_len = end
            .checked_sub(start)
            .map(|len| len / WORD_SIZE_BYTES)
            .unwrap_or(0);
        if program_words > max_len {
            return None;
        }
        self.slice_program_words(start, program_words)
    }

    fn validate_program_crc(&self, slot: usize, program_words: usize, expected_crc: u32) -> bool {
        let Some(program) = self.program_slice_for_slot(slot, program_words) else {
            return false;
        };
        crc32_words(program) == expected_crc
    }

    fn ui_state_start(&self, slot: usize, program_words: usize) -> Result<usize, StorageError> {
        let slot_start = self.slot_start_addr(slot)?;
        let program_bytes = program_words
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let raw = HEADER_SIZE_BYTES
            .checked_add(program_bytes)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let aligned = align_up(raw, F::WRITE_SIZE)?;
        slot_start
            .checked_add(aligned)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))
    }

    fn ui_state_start_offset(&self, slot: usize, program_words: usize) -> Result<u32, StorageError> {
        let slot_offset = self.slot_offset(slot)?;
        let program_bytes = program_words
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let raw = HEADER_SIZE_BYTES
            .checked_add(program_bytes)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let aligned = align_up(raw, F::WRITE_SIZE)?;
        slot_offset
            .checked_add(
                u32::try_from(aligned)
                    .map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))
    }

    fn ui_state_capacity_bytes(
        &self,
        slot: usize,
        program_words: usize,
    ) -> Result<usize, StorageError> {
        let slot_end = self.slot_end_addr(slot)?;
        let ui_start = self.ui_state_start(slot, program_words)?;
        slot_end
            .checked_sub(ui_start)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))
    }

    fn ui_state_slice_for_slot<'a>(
        &'a self,
        slot: usize,
        program_words: usize,
        ui_state_len: usize,
    ) -> Option<&'a [u8]> {
        let ui_start = self.ui_state_start(slot, program_words).ok()?;
        let slot_end = self.slot_end_addr(slot).ok()?;
        let ui_end = ui_start.checked_add(ui_state_len)?;
        if ui_end > slot_end {
            return None;
        }
        self.slice_bytes(ui_start, ui_state_len)
    }

    fn validate_ui_state_crc(
        &self,
        slot: usize,
        program_words: usize,
        ui_state_len: usize,
        expected_crc: u32,
    ) -> bool {
        if ui_state_len == 0 {
            return expected_crc == crc32_empty();
        }
        let Some(ui_state) = self.ui_state_slice_for_slot(slot, program_words, ui_state_len) else {
            return false;
        };
        crc32_bytes(ui_state) == expected_crc
    }

    fn program_slice<'a>(&'a self) -> &'a [ProgramWord] {
        let Some((start, end)) = self.program_bounds(self.active_slot) else {
            return &[];
        };
        let max_len = end
            .checked_sub(start)
            .map(|len| len / WORD_SIZE_BYTES)
            .unwrap_or(0);
        let len = self.program_words.min(max_len);
        self.slice_program_words(start, len).unwrap_or(&[])
    }

    fn program_bounds(&self, slot: usize) -> Option<(usize, usize)> {
        let slot_start = self.slot_start_addr(slot).ok()?;
        let program_start = slot_start.checked_add(HEADER_SIZE_BYTES)?;
        let program_end = slot_start.checked_add(self.slot_len)?;
        if program_start > self.storage_end || program_end > self.storage_end {
            return None;
        }
        Some((program_start, program_end))
    }

    fn program_capacity_words(&self, slot: usize) -> Option<usize> {
        let (start, end) = self.program_bounds(slot)?;
        end.checked_sub(start)
            .and_then(|len| len.checked_div(WORD_SIZE_BYTES))
    }

    fn read_header(&mut self, slot: usize) -> Result<StorageHeader, StorageError> {
        self.check_header_len()?;
        let slot_offset = self.slot_offset(slot)?;
        let mut bytes = [0u8; HEADER_SIZE_BYTES];
        self.flash
            .read(slot_offset, &mut bytes)
            .map_err(map_flash_error)?;
        let magic = read_header_u32_le(&bytes, 0..4)?;
        if magic != HEADER_MAGIC {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let version = read_header_u32_le(&bytes, 4..8)?;
        let program_words = read_header_u32_le(&bytes, 8..12)?;
        let program_crc = read_header_u32_le(&bytes, 12..16)?;
        let ui_state_len = read_header_u32_le(&bytes, 16..20)?;
        let ui_state_crc = read_header_u32_le(&bytes, 20..24)?;
        let sequence = read_header_u32_le(&bytes, 24..28)?;
        let header_crc = read_header_u32_le(&bytes, 28..32)?;
        let computed_crc = crc32_bytes(bytes.get(0..28).ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?);
        if computed_crc != header_crc {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        if version != HEADER_VERSION {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let capacity_words = self
            .program_capacity_words(slot)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let program_words_usize =
            usize::try_from(program_words).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
        if program_words_usize > capacity_words {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let ui_state_len_usize =
            usize::try_from(ui_state_len).map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
        let program_start = self.program_start_addr(slot)?;
        let program_len = program_words_usize
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let program_end = program_start
            .checked_add(program_len)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        let slot_end = self.slot_end_addr(slot)?;
        if program_end > slot_end {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        let ui_capacity = self.ui_state_capacity_bytes(slot, program_words_usize)?;
        if ui_state_len_usize > ui_capacity {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        Ok(StorageHeader {
            program_words,
            program_crc,
            ui_state_len,
            ui_state_crc,
            sequence,
        })
    }

    fn read_word(&mut self, offset: u32) -> Result<ProgramWord, StorageError> {
        let mut bytes = [0u8; WORD_SIZE_BYTES];
        self.flash.read(offset, &mut bytes).map_err(map_flash_error)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn check_header_len(&self) -> Result<usize, StorageError> {
        let storage_len = self
            .storage_end
            .checked_sub(self.storage_start)
            .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?;
        if storage_len < HEADER_SIZE_BYTES || self.slot_len < HEADER_SIZE_BYTES {
            return Err(StorageError::new(StorageErrorKind::InvalidHeader));
        }
        Ok(storage_len)
    }

    fn flash_erase_range(&mut self, start: u32, len: usize) -> Result<(), StorageError> {
        let end = start
            .checked_add(u32::try_from(len).map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        self.flash
            .erase(start, end)
            .map_err(map_flash_error)?;
        Ok(())
    }

    fn flash_program_words(&mut self, start: u32, program: &[ProgramWord]) -> Result<(), StorageError> {
        if start as usize % F::WRITE_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        if byte_len % F::WRITE_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        let mut offset = start;
        for &word in program {
            let bytes = word.to_le_bytes();
            self.flash
                .write(offset, &bytes)
                .map_err(map_flash_error)?;
            offset = offset
                .checked_add(WORD_SIZE_BYTES as u32)
                .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        }
        Ok(())
    }

    fn flash_program_bytes(
        &mut self,
        start: u32,
        bytes: &[u8],
        allow_pad: bool,
    ) -> Result<(), StorageError> {
        if start as usize % F::WRITE_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        if F::WRITE_SIZE > MAX_WRITE_BUFFER {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        let mut offset = start;
        let mut idx = 0usize;
        while idx < bytes.len() {
            let remaining = bytes.len() - idx;
            let chunk_len = remaining.min(F::WRITE_SIZE);
            if chunk_len < F::WRITE_SIZE && !allow_pad {
                return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
            }
            let mut buf = [0xFFu8; MAX_WRITE_BUFFER];
            buf[..chunk_len].copy_from_slice(&bytes[idx..idx + chunk_len]);
            let write_buf = &buf[..F::WRITE_SIZE];
            self.flash
                .write(offset, write_buf)
                .map_err(map_flash_error)?;
            offset = offset
                .checked_add(u32::try_from(F::WRITE_SIZE).map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?)
                .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
            idx = idx
                .checked_add(chunk_len)
                .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        }
        Ok(())
    }

    fn flash_program_header(
        &mut self,
        storage_start: u32,
        program_words: usize,
        program_crc: u32,
        ui_state_len: u32,
        ui_state_crc: u32,
        sequence: u32,
    ) -> Result<(), StorageError> {
        let header_words = encode_header(
            program_words,
            program_crc,
            ui_state_len,
            ui_state_crc,
            sequence,
        )?;
        self.flash_program_words(storage_start, &header_words)
    }
}

impl<F: NorFlash> Storage for FlashStorage<F> {
    type L = FlashProgramLoader;

    /// `size` is in instruction count (words).
    fn get_program_loader(
        &mut self,
        size: u32,
        ui_state_size: u32,
    ) -> Result<Self::L, StorageError> {
        let size_words: usize =
            size.try_into().map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let ui_state_size: usize =
            ui_state_size.try_into().map_err(|_| StorageError::new(StorageErrorKind::UiStateTooLarge))?;
        let target_slot = self.inactive_slot();
        let capacity_words = self
            .program_capacity_words(target_slot)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        if size_words > capacity_words {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        }

        let byte_len = size_words
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let ui_storage_len = align_up(ui_state_size, F::WRITE_SIZE)?;
        let ui_start_offset = align_up(
            HEADER_SIZE_BYTES
                .checked_add(byte_len)
                .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?,
            F::WRITE_SIZE,
        )?;
        let total_len = ui_start_offset
            .checked_add(ui_storage_len)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        if total_len > self.slot_len {
            return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
        }
        let erase_len = align_up(total_len, F::ERASE_SIZE)?;

        let slot_offset = self.slot_offset(target_slot)?;
        self.flash_erase_range(slot_offset, erase_len)?;
        let program_offset = slot_offset
            .checked_add(HEADER_SIZE_BYTES as u32)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let ui_offset = slot_offset
            .checked_add(
                u32::try_from(ui_start_offset).map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        Ok(FlashProgramLoader::new(
            program_offset,
            size_words,
            ui_offset,
            ui_state_size,
            self.active_sequence.wrapping_add(1),
            target_slot,
        ))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[ProgramWord],
    ) -> Result<(), StorageError> {
        if block_number != loader.next_block {
            return Err(StorageError::new(StorageErrorKind::UnexpectedBlock));
        }

        let Some(end_word) = loader.next_word.checked_add(block.len()) else {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        };
        if end_word > loader.program_words {
            return Err(StorageError::new(StorageErrorKind::ProgramTooLarge));
        }

        let offset = loader
            .program_start
            .checked_add(
                u32::try_from(loader.next_word * WORD_SIZE_BYTES)
                    .map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        self.flash_program_words(offset, block)?;
        for &word in block {
            loader.program_crc = crc32_update(loader.program_crc, &word.to_le_bytes());
        }

        loader.next_word = end_word;
        loader.next_block = loader
            .next_block
            .checked_add(1)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        Ok(())
    }

    fn add_ui_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[u8],
    ) -> Result<(), StorageError> {
        if block_number != loader.next_ui_block {
            return Err(StorageError::new(StorageErrorKind::UnexpectedBlock));
        }
        let Some(end_byte) = loader.next_ui_offset.checked_add(block.len()) else {
            return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
        };
        if end_byte > loader.ui_state_len {
            return Err(StorageError::new(StorageErrorKind::UiStateTooLarge));
        }
        let offset = loader
            .ui_state_start
            .checked_add(
                u32::try_from(loader.next_ui_offset).map_err(|_| StorageError::new(StorageErrorKind::UiStateTooLarge))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::UiStateTooLarge))?;
        let is_last = end_byte == loader.ui_state_len;
        if !is_last && block.len() % F::WRITE_SIZE != 0 {
            return Err(StorageError::new(StorageErrorKind::UnalignedWrite));
        }
        self.flash_program_bytes(offset, block, is_last)?;
        for &byte in block {
            loader.ui_state_crc = crc32_update(loader.ui_state_crc, &[byte]);
        }
        loader.next_ui_offset = end_byte;
        loader.next_ui_block = loader
            .next_ui_block
            .checked_add(1)
            .ok_or(StorageError::new(StorageErrorKind::UiStateTooLarge))?;
        Ok(())
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        let program_words = loader.program_words;
        if loader.next_word != program_words {
            return Err(StorageError::new(StorageErrorKind::ProgramIncomplete));
        }
        if loader.next_ui_offset != loader.ui_state_len {
            return Err(StorageError::new(StorageErrorKind::UiStateIncomplete));
        }
        let program_crc = crc32_finalize(loader.program_crc);
        let ui_state_crc = crc32_finalize(loader.ui_state_crc);
        let slot_offset = self.slot_offset(loader.target_slot)?;
        self.flash_program_header(
            slot_offset,
            program_words,
            program_crc,
            loader.ui_state_len as u32,
            ui_state_crc,
            loader.sequence,
        )?;
        self.program_words = program_words;
        self.ui_state_len = loader.ui_state_len;
        self.active_slot = loader.target_slot;
        self.active_sequence = loader.sequence;
        Ok(ProgramNumber::new(0))
    }

    fn get_program<'a, 'b>(
        &'a mut self,
        program_number: ProgramNumber,
        memory: &'b mut [StackWord],
    ) -> Result<Program<'a, 'b>, StorageError> {
        if program_number.value() != 0 {
            return Err(StorageError::new(StorageErrorKind::UnknownProgram));
        }

        let program = self.program_slice();
        Program::new(program, memory).map_err(StorageError::invalid_program)
    }

    fn get_ui_state_len(&mut self, program_number: ProgramNumber) -> Result<u32, StorageError> {
        if program_number.value() != 0 {
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
        if program_number.value() != 0 {
            return Err(StorageError::new(StorageErrorKind::UnknownProgram));
        }
        let offset_usize: usize =
            offset.try_into().map_err(|_| StorageError::new(StorageErrorKind::UiStateReadOutOfBounds))?;
        if offset_usize >= self.ui_state_len {
            return Ok(0);
        }
        let remaining = self.ui_state_len - offset_usize;
        let to_read = remaining.min(out.len());
        if to_read == 0 {
            return Ok(0);
        }
        let ui_start = self.ui_state_start_offset(self.active_slot, self.program_words)?;
        let flash_offset = ui_start
            .checked_add(
                u32::try_from(offset_usize)
                    .map_err(|_| StorageError::new(StorageErrorKind::UiStateReadOutOfBounds))?,
            )
            .ok_or(StorageError::new(StorageErrorKind::UiStateReadOutOfBounds))?;
        self.flash
            .read(
                flash_offset,
                &mut out[..to_read],
            )
            .map_err(map_flash_error)?;
        Ok(to_read)
    }
}

pub struct FlashProgramLoader {
    program_start: u32,
    program_words: usize,
    next_block: u32,
    next_word: usize,
    program_crc: u32,
    ui_state_start: u32,
    ui_state_len: usize,
    next_ui_block: u32,
    next_ui_offset: usize,
    ui_state_crc: u32,
    sequence: u32,
    target_slot: usize,
}

impl FlashProgramLoader {
    fn new(
        program_start: u32,
        program_words: usize,
        ui_state_start: u32,
        ui_state_len: usize,
        sequence: u32,
        target_slot: usize,
    ) -> Self {
        Self {
            program_start,
            program_words,
            next_block: 0,
            next_word: 0,
            program_crc: crc32_init(),
            ui_state_start,
            ui_state_len,
            next_ui_block: 0,
            next_ui_offset: 0,
            ui_state_crc: crc32_init(),
            sequence,
            target_slot,
        }
    }
}

struct StorageHeader {
    program_words: u32,
    program_crc: u32,
    ui_state_len: u32,
    ui_state_crc: u32,
    sequence: u32,
}

fn storage_bounds() -> (usize, usize) {
    // SAFETY: These are linker-provided symbols, so taking their addresses is safe and does
    // not require alignment. Alignment only matters when we later cast to `*const ProgramWord`, and
    // the storage region is defined on a flash boundary in the linker script.
    let start = unsafe { &__storage_start as *const u8 as usize };
    let end = unsafe { &__storage_end as *const u8 as usize };
    (start, end)
}

fn encode_header(
    program_words: usize,
    program_crc: u32,
    ui_state_len: u32,
    ui_state_crc: u32,
    sequence: u32,
) -> Result<[ProgramWord; HEADER_WORDS], StorageError> {
    let program_words = u32::try_from(program_words).map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?;
    let mut bytes = [0u8; HEADER_SIZE_BYTES];
    bytes
        .get_mut(0..4)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes
        .get_mut(4..8)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&HEADER_VERSION.to_le_bytes());
    bytes
        .get_mut(8..12)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&program_words.to_le_bytes());
    bytes
        .get_mut(12..16)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&program_crc.to_le_bytes());
    bytes
        .get_mut(16..20)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&ui_state_len.to_le_bytes());
    bytes
        .get_mut(20..24)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&ui_state_crc.to_le_bytes());
    bytes
        .get_mut(24..28)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&sequence.to_le_bytes());
    let header_crc = crc32_bytes(bytes.get(0..28).ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?);
    bytes
        .get_mut(28..32)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?
        .copy_from_slice(&header_crc.to_le_bytes());
    let mut words = [0u16; HEADER_WORDS];
    for (idx, chunk) in bytes.chunks_exact(WORD_SIZE_BYTES).enumerate() {
        let word_bytes = chunk.get(0..2).ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        let word = u16::from_le_bytes(
            word_bytes
                .try_into()
                .map_err(|_| StorageError::new(StorageErrorKind::ProgramTooLarge))?,
        );
        let slot = words
            .get_mut(idx)
            .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
        *slot = word;
    }
    Ok(words)
}

fn crc32_bytes(bytes: &[u8]) -> u32 {
    let crc = crc32_update(crc32_init(), bytes);
    crc32_finalize(crc)
}

fn crc32_words(words: &[ProgramWord]) -> u32 {
    let mut crc = crc32_init();
    for &word in words {
        crc = crc32_update(crc, &word.to_le_bytes());
    }
    crc32_finalize(crc)
}

fn crc32_empty() -> u32 {
    crc32_finalize(crc32_init())
}

fn crc32_init() -> u32 {
    0xFFFF_FFFFu32
}

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = if (crc & 1) == 1 { u32::MAX } else { 0 };
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & mask);
        }
    }
    crc
}

fn crc32_finalize(crc: u32) -> u32 {
    !crc
}

fn read_header_u32_le(bytes: &[u8], range: core::ops::Range<usize>) -> Result<u32, StorageError> {
    let chunk = bytes
        .get(range)
        .ok_or(StorageError::new(StorageErrorKind::InvalidHeader))?
        .try_into()
        .map_err(|_| StorageError::new(StorageErrorKind::InvalidHeader))?;
    Ok(u32::from_le_bytes(chunk))
}

fn align_up(value: usize, align: usize) -> Result<usize, StorageError> {
    let mask = align
        .checked_sub(1)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
    let value = value
        .checked_add(mask)
        .ok_or(StorageError::new(StorageErrorKind::ProgramTooLarge))?;
    Ok(value & !mask)
}

fn is_seq_newer(candidate: u32, current: u32) -> bool {
    candidate != current && candidate.wrapping_sub(current) < 0x8000_0000
}

fn map_flash_error<E: NorFlashError>(error: E) -> StorageError {
    match error.kind() {
        NorFlashErrorKind::NotAligned => StorageError::new(StorageErrorKind::UnalignedWrite),
        NorFlashErrorKind::OutOfBounds => StorageError::new(StorageErrorKind::ProgramTooLarge),
        NorFlashErrorKind::Other => StorageError::new(StorageErrorKind::WriteFailed),
        _ => StorageError::new(StorageErrorKind::WriteFailed),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use embedded_storage::nor_flash::{
        check_erase, check_read, check_write, ErrorType, NorFlash, NorFlashErrorKind, ReadNorFlash,
    };

    const READ_SIZE: usize = 1;
    const WRITE_SIZE: usize = 2;
    const ERASE_SIZE: usize = 64;
    const FLASH_BYTES: usize = 4096;
    const FLASH_WORDS: usize = FLASH_BYTES / WORD_SIZE_BYTES;

    fn program_words_as_bytes(storage: &[ProgramWord]) -> &[u8] {
        let len = storage.len() * WORD_SIZE_BYTES;
        // SAFETY: ProgramWord is 2 bytes and we only re-interpret as raw bytes.
        unsafe { core::slice::from_raw_parts(storage.as_ptr() as *const u8, len) }
    }

    fn program_words_as_bytes_mut(storage: &mut [ProgramWord]) -> &mut [u8] {
        let len = storage.len() * WORD_SIZE_BYTES;
        // SAFETY: ProgramWord is 2 bytes and we only re-interpret as raw bytes.
        unsafe { core::slice::from_raw_parts_mut(storage.as_mut_ptr() as *mut u8, len) }
    }

    struct MockFlash {
        storage: std::vec::Vec<ProgramWord>,
    }

    impl MockFlash {
        fn new(words: usize) -> Self {
            Self {
                storage: std::vec![0xFFFF; words],
            }
        }

        fn bytes(&self) -> &[u8] {
            program_words_as_bytes(&self.storage)
        }

        fn bytes_mut(&mut self) -> &mut [u8] {
            program_words_as_bytes_mut(&mut self.storage)
        }

        fn corrupt_byte(&mut self, offset: usize) {
            let bytes = self.bytes_mut();
            if let Some(slot) = bytes.get_mut(offset) {
                *slot ^= 0x01;
            }
        }
    }

    impl ErrorType for MockFlash {
        type Error = NorFlashErrorKind;
    }

    impl ReadNorFlash for MockFlash {
        const READ_SIZE: usize = READ_SIZE;

        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
            check_read(self, offset, bytes.len())?;
            let start = offset as usize;
            let end = start + bytes.len();
            bytes.copy_from_slice(&self.bytes()[start..end]);
            Ok(())
        }

        fn capacity(&self) -> usize {
            self.storage.len() * WORD_SIZE_BYTES
        }
    }

    impl NorFlash for MockFlash {
        const WRITE_SIZE: usize = WRITE_SIZE;
        const ERASE_SIZE: usize = ERASE_SIZE;

        fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
            check_erase(self, from, to)?;
            let start = from as usize;
            let end = to as usize;
            for byte in &mut self.bytes_mut()[start..end] {
                *byte = 0xFF;
            }
            Ok(())
        }

        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
            check_write(self, offset, bytes.len())?;
            let start = offset as usize;
            let data = self.bytes_mut();
            for (idx, &value) in bytes.iter().enumerate() {
                let slot = &mut data[start + idx];
                if *slot != 0xFF {
                    return Err(NorFlashErrorKind::Other);
                }
                *slot = value;
            }
            Ok(())
        }
    }

    fn make_storage() -> FlashStorage<MockFlash> {
        let flash = MockFlash::new(FLASH_WORDS);
        let storage_start = flash.storage.as_ptr() as usize;
        let storage_end = storage_start + FLASH_BYTES;
        FlashStorage::new_with_bounds(flash, storage_start, storage_start, storage_end)
            .expect("storage init")
    }

    #[test]
    fn load_header_picks_newest_valid_slot() {
        let mut storage = make_storage();
        storage.format().expect("format");

        let program_a = [0x1111u16, 0x2222, 0x3333];
        storage.write_program(&program_a).expect("write a");
        assert_eq!(storage.active_slot, 1);
        assert_eq!(storage.active_sequence, 1);

        let program_b = [0x4444u16, 0x5555];
        storage.write_program(&program_b).expect("write b");
        assert_eq!(storage.active_slot, 0);
        assert_eq!(storage.active_sequence, 2);

        let header0 = storage.read_header(0).expect("header0");
        let header1 = storage.read_header(1).expect("header1");
        assert_eq!(header0.sequence, 2);
        assert_eq!(header1.sequence, 1);
        assert!(storage.validate_program_crc(
            0,
            header0.program_words as usize,
            header0.program_crc
        ));
        assert!(storage.validate_program_crc(
            1,
            header1.program_words as usize,
            header1.program_crc
        ));

        storage.load_header().expect("load");
        assert_eq!(storage.program_words, program_b.len());
        assert_eq!(storage.active_sequence, 2);
        assert_eq!(storage.active_slot, 0);
    }

    #[test]
    fn load_header_ignores_corrupt_newer_slot() {
        let mut storage = make_storage();
        storage.format().expect("format");

        let program_a = [0x1111u16, 0x2222, 0x3333];
        storage.write_program(&program_a).expect("write a");

        let program_b = [0x4444u16, 0x5555];
        storage.write_program(&program_b).expect("write b");

        let corrupt_slot = storage.active_slot;
        let slot_offset = storage.slot_offset(corrupt_slot).expect("slot offset") as usize;
        storage
            .flash
            .corrupt_byte(slot_offset + HEADER_SIZE_BYTES);

        storage.load_header().expect("load");
        assert_eq!(storage.program_words, program_a.len());
        assert_eq!(storage.active_sequence, 1);
        assert_eq!(storage.active_slot, 1);
    }

    #[test]
    fn finish_load_requires_complete_program() {
        let mut storage = make_storage();
        storage.format().expect("format");

        let mut loader = storage.get_program_loader(4, 0).expect("loader");
        storage
            .add_block(&mut loader, 0, &[0xAAAAu16, 0xBBBB])
            .expect("block");

        assert!(matches!(
            storage.finish_load(loader),
            Err(err) if err.kind() == StorageErrorKind::ProgramIncomplete
        ));
    }

    #[test]
    fn load_does_not_flip_active_slot_until_finish() {
        let mut storage = make_storage();
        storage.format().expect("format");

        let program_a = [0x1111u16, 0x2222, 0x3333];
        storage.write_program(&program_a).expect("write a");

        let prev_slot = storage.active_slot;
        let prev_seq = storage.active_sequence;

        let mut loader = storage.get_program_loader(4, 0).expect("loader");
        storage
            .add_block(&mut loader, 0, &[0xAAAAu16, 0xBBBB])
            .expect("block");

        assert_eq!(storage.active_slot, prev_slot);
        assert_eq!(storage.active_sequence, prev_seq);
    }
}
