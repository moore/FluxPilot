//! Flash-backed storage for Pliot programs using embedded-storage flash traits.
use embedded_storage::nor_flash::{NorFlash, NorFlashError, NorFlashErrorKind};
use light_machine::{Program, Word};
use pliot::{ProgramNumber, Storage, StorageError};

const WORD_SIZE_BYTES: usize = 2;
// Header layout: magic, version, program length (words), header crc32.
const HEADER_MAGIC: u32 = u32::from_le_bytes(*b"PLIO");
const HEADER_VERSION: u32 = 1;
const HEADER_SIZE_BYTES: usize = 16;
const HEADER_WORDS: usize = HEADER_SIZE_BYTES / WORD_SIZE_BYTES;

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
            .ok_or(StorageError::InvalidHeader)?;
        if storage_len < HEADER_SIZE_BYTES {
            return Err(StorageError::InvalidHeader);
        }
        let storage_offset = storage_start
            .checked_sub(flash_base)
            .ok_or(StorageError::InvalidHeader)?;
        let storage_offset = u32::try_from(storage_offset).map_err(|_| StorageError::InvalidHeader)?;
        if storage_offset as usize % F::READ_SIZE != 0
            || storage_offset as usize % F::WRITE_SIZE != 0
            || storage_offset as usize % F::ERASE_SIZE != 0
        {
            return Err(StorageError::UnalignedWrite);
        }
        if WORD_SIZE_BYTES % F::WRITE_SIZE != 0 {
            return Err(StorageError::UnalignedWrite);
        }
        if HEADER_SIZE_BYTES % F::WRITE_SIZE != 0 || HEADER_SIZE_BYTES % F::READ_SIZE != 0 {
            return Err(StorageError::UnalignedWrite);
        }
        let storage_end_offset = storage_offset
            .checked_add(
                u32::try_from(storage_len).map_err(|_| StorageError::InvalidHeader)?,
            )
            .ok_or(StorageError::InvalidHeader)?;
        if storage_end_offset as usize > flash.capacity() {
            return Err(StorageError::InvalidHeader);
        }
        Ok(Self {
            flash,
            storage_start,
            storage_end,
            storage_offset,
            program_words: 0,
        })
    }

    pub fn load_header(&mut self) -> Result<(), StorageError> {
        let header = self.read_header()?;
        let program_words =
            usize::try_from(header.program_words).map_err(|_| StorageError::InvalidHeader)?;
        self.program_words = program_words;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.program_words == 0
    }

    pub fn probe_write_read(&mut self) -> Result<(), StorageError> {
        const TEST_VALUE: Word = 0xA5A5;
        let offset = self.storage_offset;
        let erase_len = align_up(WORD_SIZE_BYTES, F::ERASE_SIZE)?;
        self.flash_erase_range(offset, erase_len)?;

        let erased = self.read_word(offset)?;
        if erased != 0xFFFF {
            return Err(StorageError::InvalidHeader);
        }

        self.flash_program_words(offset, &[TEST_VALUE])?;
        let read_value = self.read_word(offset)?;
        if read_value != TEST_VALUE {
            return Err(StorageError::InvalidHeader);
        }
        Ok(())
    }

    pub fn format(&mut self) -> Result<(), StorageError> {
        let erase_len = align_up(HEADER_SIZE_BYTES, F::ERASE_SIZE)?;
        self.flash_erase_range(self.storage_offset, erase_len)?;
        self.flash_program_header(self.storage_offset, 0)?;
        self.load_header()
    }

    pub fn write_program(&mut self, program: &[Word]) -> Result<(), StorageError> {
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let program_start_addr = self
            .storage_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if program_start_addr > self.storage_end {
            return Err(StorageError::ProgramTooLarge);
        }
        let capacity = self
            .storage_end
            .checked_sub(program_start_addr)
            .ok_or(StorageError::ProgramTooLarge)?;
        if byte_len > capacity {
            return Err(StorageError::ProgramTooLarge);
        }

        let total_len = byte_len
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let erase_len = align_up(total_len, F::ERASE_SIZE)?;

        self.flash_erase_range(self.storage_offset, erase_len)?;
        let program_offset = self
            .storage_offset
            .checked_add(HEADER_SIZE_BYTES as u32)
            .ok_or(StorageError::ProgramTooLarge)?;
        self.flash_program_words(program_offset, program)?;
        self.flash_program_header(self.storage_offset, program.len())?;

        self.program_words = program.len();
        Ok(())
    }

    fn program_slice<'a>(&'a self) -> &'a [Word] {
        let Some((start, end)) = self.program_bounds() else {
            return &[];
        };
        let max_len = end
            .checked_sub(start)
            .map(|len| len / WORD_SIZE_BYTES)
            .unwrap_or(0);
        let len = self.program_words.min(max_len);
        // SAFETY: The storage region is linker-defined, word-aligned, and lives for the
        // program lifetime, so it's valid to create a static slice over it.
        unsafe { core::slice::from_raw_parts(start as *const Word, len) }
    }

    fn program_bounds(&self) -> Option<(usize, usize)> {
        let program_start = self.storage_start.checked_add(HEADER_SIZE_BYTES)?;
        if program_start > self.storage_end {
            return None;
        }
        Some((program_start, self.storage_end))
    }

    fn program_capacity_words(&self) -> Option<usize> {
        let (start, end) = self.program_bounds()?;
        end.checked_sub(start)
            .and_then(|len| len.checked_div(WORD_SIZE_BYTES))
    }

    fn read_header(&mut self) -> Result<StorageHeader, StorageError> {
        self.check_header_len()?;
        let mut bytes = [0u8; HEADER_SIZE_BYTES];
        self.flash
            .read(self.storage_offset, &mut bytes)
            .map_err(map_flash_error)?;
        let magic = read_header_u32_le(&bytes, 0..4)?;
        if magic != HEADER_MAGIC {
            return Err(StorageError::InvalidHeader);
        }
        let version = read_header_u32_le(&bytes, 4..8)?;
        let program_words = read_header_u32_le(&bytes, 8..12)?;
        let header_crc = read_header_u32_le(&bytes, 12..16)?;
        let computed_crc = crc32_bytes(bytes.get(0..12).ok_or(StorageError::InvalidHeader)?);
        if computed_crc != header_crc {
            return Err(StorageError::InvalidHeader);
        }
        if version != HEADER_VERSION {
            return Err(StorageError::InvalidHeader);
        }
        let capacity_words = self.program_capacity_words().ok_or(StorageError::InvalidHeader)?;
        let program_words_usize =
            usize::try_from(program_words).map_err(|_| StorageError::InvalidHeader)?;
        if program_words_usize > capacity_words {
            return Err(StorageError::InvalidHeader);
        }
        let program_start = self
            .storage_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::InvalidHeader)?;
        let program_len = program_words_usize
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::InvalidHeader)?;
        let program_end = program_start
            .checked_add(program_len)
            .ok_or(StorageError::InvalidHeader)?;
        if program_end > self.storage_end {
            return Err(StorageError::InvalidHeader);
        }
        Ok(StorageHeader {
            version,
            program_words,
            header_crc,
        })
    }

    fn read_word(&mut self, offset: u32) -> Result<Word, StorageError> {
        let mut bytes = [0u8; WORD_SIZE_BYTES];
        self.flash.read(offset, &mut bytes).map_err(map_flash_error)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn check_header_len(&self) -> Result<usize, StorageError> {
        let storage_len = self
            .storage_end
            .checked_sub(self.storage_start)
            .ok_or(StorageError::InvalidHeader)?;
        if storage_len < HEADER_SIZE_BYTES {
            return Err(StorageError::InvalidHeader);
        }
        Ok(storage_len)
    }

    fn flash_erase_range(&mut self, start: u32, len: usize) -> Result<(), StorageError> {
        let end = start
            .checked_add(u32::try_from(len).map_err(|_| StorageError::ProgramTooLarge)?)
            .ok_or(StorageError::ProgramTooLarge)?;
        self.flash
            .erase(start, end)
            .map_err(map_flash_error)?;
        Ok(())
    }

    fn flash_program_words(&mut self, start: u32, program: &[Word]) -> Result<(), StorageError> {
        if start as usize % F::WRITE_SIZE != 0 {
            return Err(StorageError::UnalignedWrite);
        }
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if byte_len % F::WRITE_SIZE != 0 {
            return Err(StorageError::UnalignedWrite);
        }
        let mut offset = start;
        for &word in program {
            let bytes = word.to_le_bytes();
            self.flash
                .write(offset, &bytes)
                .map_err(map_flash_error)?;
            offset = offset
                .checked_add(WORD_SIZE_BYTES as u32)
                .ok_or(StorageError::ProgramTooLarge)?;
        }
        Ok(())
    }

    fn flash_program_header(
        &mut self,
        storage_start: u32,
        program_words: usize,
    ) -> Result<(), StorageError> {
        let header_words = encode_header(program_words)?;
        self.flash_program_words(storage_start, &header_words)
    }
}

impl<F: NorFlash> Storage for FlashStorage<F> {
    type L = FlashProgramLoader;

    /// `size` is in instruction count (words).
    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError> {
        let size_words: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;
        let program_start_addr = self
            .storage_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if program_start_addr > self.storage_end {
            return Err(StorageError::ProgramTooLarge);
        }
        let capacity_words = self
            .storage_end
            .checked_sub(program_start_addr)
            .ok_or(StorageError::ProgramTooLarge)?
            .checked_div(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if size_words > capacity_words {
            return Err(StorageError::ProgramTooLarge);
        }

        let byte_len = size_words
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let total_len = byte_len
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let erase_len = align_up(total_len, F::ERASE_SIZE)?;

        self.flash_erase_range(self.storage_offset, erase_len)?;
        self.program_words = 0;
        let program_offset = self
            .storage_offset
            .checked_add(HEADER_SIZE_BYTES as u32)
            .ok_or(StorageError::ProgramTooLarge)?;
        Ok(FlashProgramLoader::new(program_offset, size_words))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError> {
        if block_number != loader.next_block {
            return Err(StorageError::UnexpectedBlock);
        }

        let Some(end_word) = loader.next_word.checked_add(block.len()) else {
            return Err(StorageError::ProgramTooLarge);
        };
        if end_word > loader.program_words {
            return Err(StorageError::ProgramTooLarge);
        }

        let offset = loader
            .program_start
            .checked_add(
                u32::try_from(loader.next_word * WORD_SIZE_BYTES)
                    .map_err(|_| StorageError::ProgramTooLarge)?,
            )
            .ok_or(StorageError::ProgramTooLarge)?;
        self.flash_program_words(offset, block)?;

        loader.next_word = end_word;
        loader.next_block = loader
            .next_block
            .checked_add(1)
            .ok_or(StorageError::ProgramTooLarge)?;
        Ok(())
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        let program_words = loader.program_words;
        self.flash_program_header(self.storage_offset, program_words)?;
        self.program_words = program_words;
        Ok(ProgramNumber::new(0))
    }

    fn get_program<'a, 'b>(
        &'a mut self,
        program_number: ProgramNumber,
        globals: &'b mut [Word],
    ) -> Result<Program<'a, 'b>, StorageError> {
        if program_number.value() != 0 {
            return Err(StorageError::UnknownProgram);
        }

        let program = self.program_slice();
        Program::new(program, globals).map_err(StorageError::InvalidProgram)
    }
}

pub struct FlashProgramLoader {
    program_start: u32,
    program_words: usize,
    next_block: u32,
    next_word: usize,
}

impl FlashProgramLoader {
    fn new(program_start: u32, program_words: usize) -> Self {
        Self {
            program_start,
            program_words,
            next_block: 0,
            next_word: 0,
        }
    }
}

struct StorageHeader {
    version: u32,
    program_words: u32,
    header_crc: u32,
}

fn storage_bounds() -> (usize, usize) {
    // SAFETY: These are linker-provided symbols, so taking their addresses is safe and does
    // not require alignment. Alignment only matters when we later cast to `*const Word`, and
    // the storage region is defined on a flash boundary in the linker script.
    let start = unsafe { &__storage_start as *const u8 as usize };
    let end = unsafe { &__storage_end as *const u8 as usize };
    (start, end)
}

fn encode_header(program_words: usize) -> Result<[Word; HEADER_WORDS], StorageError> {
    let program_words = u32::try_from(program_words).map_err(|_| StorageError::ProgramTooLarge)?;
    let mut bytes = [0u8; HEADER_SIZE_BYTES];
    bytes
        .get_mut(0..4)
        .ok_or(StorageError::ProgramTooLarge)?
        .copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes
        .get_mut(4..8)
        .ok_or(StorageError::ProgramTooLarge)?
        .copy_from_slice(&HEADER_VERSION.to_le_bytes());
    bytes
        .get_mut(8..12)
        .ok_or(StorageError::ProgramTooLarge)?
        .copy_from_slice(&program_words.to_le_bytes());
    let header_crc = crc32_bytes(bytes.get(0..12).ok_or(StorageError::ProgramTooLarge)?);
    bytes
        .get_mut(12..16)
        .ok_or(StorageError::ProgramTooLarge)?
        .copy_from_slice(&header_crc.to_le_bytes());
    let mut words = [0u16; HEADER_WORDS];
    for (idx, chunk) in bytes.chunks_exact(WORD_SIZE_BYTES).enumerate() {
        let word_bytes = chunk.get(0..2).ok_or(StorageError::ProgramTooLarge)?;
        let word = u16::from_le_bytes(
            word_bytes
                .try_into()
                .map_err(|_| StorageError::ProgramTooLarge)?,
        );
        let slot = words
            .get_mut(idx)
            .ok_or(StorageError::ProgramTooLarge)?;
        *slot = word;
    }
    Ok(words)
}

fn crc32_bytes(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = if (crc & 1) == 1 { u32::MAX } else { 0 };
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & mask);
        }
    }
    !crc
}

fn read_header_u32_le(bytes: &[u8], range: core::ops::Range<usize>) -> Result<u32, StorageError> {
    let chunk = bytes
        .get(range)
        .ok_or(StorageError::InvalidHeader)?
        .try_into()
        .map_err(|_| StorageError::InvalidHeader)?;
    Ok(u32::from_le_bytes(chunk))
}

fn align_up(value: usize, align: usize) -> Result<usize, StorageError> {
    let mask = align
        .checked_sub(1)
        .ok_or(StorageError::ProgramTooLarge)?;
    let value = value
        .checked_add(mask)
        .ok_or(StorageError::ProgramTooLarge)?;
    Ok(value & !mask)
}

fn map_flash_error<E: NorFlashError>(error: E) -> StorageError {
    match error.kind() {
        NorFlashErrorKind::NotAligned => StorageError::UnalignedWrite,
        NorFlashErrorKind::OutOfBounds => StorageError::ProgramTooLarge,
        NorFlashErrorKind::Other => StorageError::WriteFailed,
        _ => StorageError::WriteFailed,
    }
}
