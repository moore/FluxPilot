//! Flash-backed storage for Pliot programs.
//!
//! Overview:
//! - The storage region is a linker-defined flash window (`__storage_start`/`__storage_end`).
//! - Reads are performed by creating a slice over that region and passing it to the VM.
//! - Writes use the CH32 FPEC controller with unlock/lock sequencing and explicit
//!   erase/program operations.
//!
//! Write flow:
//! 1) Exit enhanced read mode before any erase/program (CH32FV2x_V3x RM 32.3).
//! 2) Unlock the FPEC by writing KEY1/KEY2 to FLASH_KEYR (RM 32.4.1).
//! 3) If nessary divide HCLK by 2 during flash ops so flash access clock <= 60 MHz
//!    when SYSCLK > 120 MHz (RM 32.2 note).
//! 4) Erase the required pages (page size 256 bytes in fast mode, RM 32.2.1 and 32.5.3).
//! 5) Program halfwords (2-byte writes) with PG set (RM 32.5.3).
//! 6) Lock the FPEC again and re-enter enhanced read mode (RM 32.3).
//!
//! Enhanced read mode:
//! - Enable by setting EHMOD, then wait for EHMODS (RM 32.3).
//! - Disable by clearing EHMOD and setting RSENACT, then wait for EHMODS to clear (RM 32.3).
//!
//! Clock management:
//! - When SYSCLK > 120 MHz, the manual recommends HCLK/2 during flash operations
//!   so flash access clock <= 60 MHz (RM 32.2 note).
//! - `FlashClockGuard` temporarily sets HPRE=DIV2 and restores it afterward.
//!
//! References:
//! - CH32FV2x_V3x Reference Manual, sections 32.2, 32.3, 32.4, 32.5.
use core::ptr::write_volatile;
use core::sync::atomic::{fence, Ordering};

use ch32_hal::pac;
use light_machine::{Program, Word};
use pliot::{ProgramNumber, Storage, StorageError};

use crate::debug_mark;


const PAGE_SIZE_BYTES: usize = 4096;
const WORD_SIZE_BYTES: usize = 2;
// Header layout: magic, version, start offset (words), program length (words), header crc32.
const HEADER_MAGIC: u32 = u32::from_le_bytes(*b"PLIO");
const HEADER_VERSION: u32 = 1;
const HEADER_SIZE_BYTES: usize = 20;
const HEADER_WORDS: usize = HEADER_SIZE_BYTES / WORD_SIZE_BYTES;
// TODO: Confirm these keys in the datasheet section for the FLASH interface.
const FLASH_KEY1: u32 = 0x4567_0123;
const FLASH_KEY2: u32 = 0xCDEF_89AB;

extern "C" {
    static __storage_start: u8;
    static __storage_end: u8;
}

pub struct FlashStorage {
    program_words: usize,
}

impl FlashStorage {
    /// Open storage by reading and validating the on-flash header.
    pub fn open() -> Result<Self, StorageError> {
        let header = read_header()?;
        let program_words =
            usize::try_from(header.program_words).map_err(|_| StorageError::InvalidHeader)?;
        Ok(Self { program_words })
    }

    pub fn is_empty(&self) -> bool {
        self.program_words == 0
    }

    pub fn probe_write_read() -> Result<(), StorageError> {
        const TEST_VALUE: Word = 0xA5A5;
        let (storage_start, _) = storage_bounds();
        let mut probe_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            probe_result = flash_erase_range(storage_start, PAGE_SIZE_BYTES);
            flash_lock();
        });
        probe_result?;


        let erased_value = unsafe { core::ptr::read_volatile(storage_start as *const Word) };
        if erased_value != 0xe339 {
            debug_mark((64, 0, 0).into()); // Next LED (red): probe erase failed.
            return Err(StorageError::InvalidHeader);
        }
        debug_mark((0, 0, 32).into()); // Next LED (blue): probe erased ok.

        let mut write_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            debug_mark((0, 32, 32).into()); // Next LED (blue): probe erased ok.
            write_result = flash_program_word(storage_start, TEST_VALUE);
            debug_mark((0, 32, 32).into()); // Next LED (blue): probe erased ok.
            flash_lock();
            debug_mark((0, 32, 32).into()); // Next LED (blue): probe erased ok.
        });
        debug_mark((32, 0, 32).into());
        write_result?;
        debug_mark((32, 0, 32).into());

        let read_value = unsafe { core::ptr::read_volatile(storage_start as *const Word) };
        if read_value == TEST_VALUE {
            debug_mark((0, 32, 0).into()); // Next LED (green): probe readback ok.
            Ok(())
        } else {
            debug_mark((32, 0, 0).into()); // Next LED (red): probe readback mismatch.
            //debug_mark_word_bits(read_value);
            Err(StorageError::InvalidHeader)
        }

   }

    /// Write an empty header so the flash is considered formatted.
    pub fn format() -> Result<(), StorageError> {
        let (storage_start, _) = storage_bounds();
        let erase_len = align_up(HEADER_SIZE_BYTES, PAGE_SIZE_BYTES)?;
        let mut format_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            format_result = flash_erase_range(storage_start, erase_len)
                .and_then(|_| {
                    flash_program_header(storage_start, 0)
                });
            flash_lock();
        });
        format_result?;
        if read_header().is_err() {
            return Err(StorageError::InvalidHeader);
        }
        Ok(())
    }

    pub fn write_program(&mut self, program: &[Word]) -> Result<(), StorageError> {
        // Treat the input slice as the complete program image.
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let (storage_start, storage_end) = storage_bounds();
        let program_start = storage_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if program_start > storage_end {
            return Err(StorageError::ProgramTooLarge);
        }
        let capacity = storage_end
            .checked_sub(program_start)
            .ok_or(StorageError::ProgramTooLarge)?;
        if byte_len > capacity {
            return Err(StorageError::ProgramTooLarge);
        }

        let total_len = byte_len
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let erase_len = align_up(total_len, PAGE_SIZE_BYTES)?;

        let mut program_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            program_result = flash_erase_range(storage_start, erase_len)
                .and_then(|_| flash_program_words(program_start, program))
                .and_then(|_| flash_program_header(storage_start, program.len()));
            flash_lock();
        });
        program_result?;

        self.program_words = program.len();
        Ok(())
    }

    fn program_slice<'a>(&'a self) -> &'a [Word] {
        let Some((start, end)) = program_bounds() else {
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
}

impl Storage for FlashStorage {
    type L = FlashProgramLoader;

    /// `size` is in instruction count (words).
    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError> {
        let size_words: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;
        let (storage_start, storage_end) = storage_bounds();
        let program_start = storage_start
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if program_start > storage_end {
            return Err(StorageError::ProgramTooLarge);
        }
        let capacity_words = storage_end
            .checked_sub(program_start)
            .ok_or(StorageError::ProgramTooLarge)?
            .checked_div(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        if size_words > capacity_words {
            return Err(StorageError::ProgramTooLarge);
        }

        // Erase is done up front so add_block can stream writes without
        // worrying about page alignment.
        let byte_len = size_words
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let total_len = byte_len
            .checked_add(HEADER_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let erase_len = align_up(total_len, PAGE_SIZE_BYTES)?;

        let mut erase_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            erase_result = flash_erase_range(storage_start, erase_len);
            flash_lock();
        });
        erase_result?;

        self.program_words = 0;
        Ok(FlashProgramLoader::new(program_start, size_words))
    }

    fn add_block(
        &mut self,
        loader: &mut Self::L,
        block_number: u32,
        block: &[Word],
    ) -> Result<(), StorageError> {
        loader.add_block(block_number, block)
    }

    fn finish_load(&mut self, loader: Self::L) -> Result<ProgramNumber, StorageError> {
        let (storage_start, _) = storage_bounds();
        let program_words = loader.program_words;
        loader.finish_load()?;
        let mut header_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            header_result = flash_program_header(storage_start, program_words);
            flash_lock();
        });
        header_result?;

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
    program_start: usize,
    program_words: usize,
    next_block: u32,
    next_word: usize,
}

impl FlashProgramLoader {
    fn new(program_start: usize, program_words: usize) -> Self {
        Self {
            program_start,
            program_words,
            next_block: 0,
            next_word: 0,
        }
    }

    fn add_block(&mut self, block_number: u32, block: &[Word]) -> Result<(), StorageError> {
        if block_number != self.next_block {
            return Err(StorageError::UnexpectedBlock);
        }

        let Some(end_word) = self.next_word.checked_add(block.len()) else {
            return Err(StorageError::ProgramTooLarge);
        };
        if end_word > self.program_words {
            return Err(StorageError::ProgramTooLarge);
        }

        let mut next_word = self.next_word;
        let mut program_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            let _clock_guard = FlashClockGuard::enter();
            flash_unlock();
            for word in block {
                if program_result.is_err() {
                    break;
                }
                let addr = match next_word.checked_mul(WORD_SIZE_BYTES) {
                    Some(offset) => match self.program_start.checked_add(offset) {
                        Some(addr) => addr,
                        None => {
                            program_result = Err(StorageError::ProgramTooLarge);
                            break;
                        }
                    },
                    None => {
                        program_result = Err(StorageError::ProgramTooLarge);
                        break;
                    }
                };
                program_result = flash_program_word(addr, *word);
                match next_word.checked_add(1) {
                    Some(updated) => next_word = updated,
                    None => {
                        program_result = Err(StorageError::ProgramTooLarge);
                        break;
                    }
                };
            }
            flash_lock();
        });
        program_result?;

        self.next_word = next_word;
        self.next_block = self
            .next_block
            .checked_add(1)
            .ok_or(StorageError::ProgramTooLarge)?;
        Ok(())
    }

    fn finish_load(self) -> Result<ProgramNumber, StorageError> {
        Ok(ProgramNumber::new(0))
    }
}

struct StorageHeader {
    version: u32,
    start_words: u32,
    program_words: u32,
    header_crc: u32,
}

fn storage_bounds() -> (usize, usize) {
    // SAFETY: These are linker-provided symbols, so taking their addresses is safe and does
    // not require alignment. Alignment only matters when we later cast to `*const Word`, and
    // the storage region is defined on a flash boundary in the linker script.
    let start =  unsafe { &__storage_start as *const u8 as usize };
    let end = unsafe { &__storage_end as *const u8 as usize };
    (start, end)
}

fn program_bounds() -> Option<(usize, usize)> {
    let (start, end) = storage_bounds();
    let program_start = start.checked_add(HEADER_SIZE_BYTES)?;
    if program_start > end {
        return None;
    }
    Some((program_start, end))
}

fn program_capacity_words() -> Option<usize> {
    let (start, end) = program_bounds()?;
    end.checked_sub(start)
        .and_then(|len| len.checked_div(WORD_SIZE_BYTES))
}

fn read_header() -> Result<StorageHeader, StorageError> {
    let (start, end) = storage_bounds();
    if end
        .checked_sub(start)
        .ok_or(StorageError::InvalidHeader)?
        < HEADER_SIZE_BYTES
    {
        return Err(StorageError::InvalidHeader);
    }
    // SAFETY: We only read from a linker-defined flash region as bytes.
    let bytes = unsafe { core::slice::from_raw_parts(start as *const u8, HEADER_SIZE_BYTES) };
    let magic = u32::from_le_bytes(
        bytes
            .get(0..4)
            .ok_or(StorageError::InvalidHeader)?
            .try_into()
            .map_err(|_| StorageError::InvalidHeader)?,
    );
    if magic != HEADER_MAGIC {

        return Err(StorageError::InvalidHeader);
    }
    let version = u32::from_le_bytes(
        bytes
            .get(4..8)
            .ok_or(StorageError::InvalidHeader)?
            .try_into()
            .map_err(|_| StorageError::InvalidHeader)?,
    );
    let start_words = u32::from_le_bytes(
        bytes
            .get(8..12)
            .ok_or(StorageError::InvalidHeader)?
            .try_into()
            .map_err(|_| StorageError::InvalidHeader)?,
    );
    let program_words = u32::from_le_bytes(
        bytes
            .get(12..16)
            .ok_or(StorageError::InvalidHeader)?
            .try_into()
            .map_err(|_| StorageError::InvalidHeader)?,
    );
    let header_crc = u32::from_le_bytes(
        bytes
            .get(16..20)
            .ok_or(StorageError::InvalidHeader)?
            .try_into()
            .map_err(|_| StorageError::InvalidHeader)?,
    );
    let computed_crc = crc32_bytes(bytes.get(0..16).ok_or(StorageError::InvalidHeader)?);
    if computed_crc != header_crc {
        return Err(StorageError::InvalidHeader);
    }
    if version != HEADER_VERSION {
        return Err(StorageError::InvalidHeader);
    }
    if start_words != u32::try_from(HEADER_WORDS).map_err(|_| StorageError::InvalidHeader)? {
        return Err(StorageError::InvalidHeader);
    }
    let capacity_words = program_capacity_words().ok_or(StorageError::InvalidHeader)?;
    let program_words_usize =
        usize::try_from(program_words).map_err(|_| StorageError::InvalidHeader)?;
    if program_words_usize > capacity_words {
        return Err(StorageError::InvalidHeader);
    }
    let program_start = start
        .checked_add(HEADER_SIZE_BYTES)
        .ok_or(StorageError::InvalidHeader)?;
    let program_len = program_words_usize
        .checked_mul(WORD_SIZE_BYTES)
        .ok_or(StorageError::InvalidHeader)?;
    let program_end = program_start
        .checked_add(program_len)
        .ok_or(StorageError::InvalidHeader)?;
    if program_end > end {
        return Err(StorageError::InvalidHeader);
    }
    Ok(StorageHeader {
        version,
        start_words,
        program_words,
        header_crc,
    })
}

fn encode_header(
    program_words: usize,
) -> Result<[Word; HEADER_WORDS], StorageError> {
    let program_words = u32::try_from(program_words).map_err(|_| StorageError::ProgramTooLarge)?;
    let start_words = u32::try_from(HEADER_WORDS).map_err(|_| StorageError::ProgramTooLarge)?;
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
        .copy_from_slice(&start_words.to_le_bytes());
    bytes
        .get_mut(12..16)
        .ok_or(StorageError::ProgramTooLarge)?
        .copy_from_slice(&program_words.to_le_bytes());
    let header_crc = crc32_bytes(bytes.get(0..16).ok_or(StorageError::ProgramTooLarge)?);
    bytes
        .get_mut(16..20)
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

fn align_up(value: usize, align: usize) -> Result<usize, StorageError> {
    let mask = align
        .checked_sub(1)
        .ok_or(StorageError::ProgramTooLarge)?;
    let value = value
        .checked_add(mask)
        .ok_or(StorageError::ProgramTooLarge)?;
    Ok(value & !mask)
}

fn flash_unlock() {
    //let flash = pac::FLASH;
    let _clock_guard = FlashClockGuard::enter();
    // Enhanced read mode must be disabled before any erase/program sequence.
    flash_exit_enhanced_read();
    // if flash.ctlr().read().lock() {
    //     flash.keyr().write(|w| w.set_keyr(FLASH_KEY1));
    //     flash.keyr().write(|w| w.set_keyr(FLASH_KEY2));
    // }
    if pac::FLASH.ctlr().read().lock() {
        pac::FLASH.keyr().write(|w| w.set_keyr(0x4567_0123));
        fence(Ordering::SeqCst);
        pac::FLASH.keyr().write(|w| w.set_keyr(0xCDEF_89AB));
        fence(Ordering::SeqCst);
    }
    if pac::FLASH.ctlr().read().flock() {
        pac::FLASH.modekeyr().write(|w| w.set_modekeyr(0x4567_0123));
        fence(Ordering::SeqCst);
        pac::FLASH.modekeyr().write(|w| w.set_modekeyr(0xCDEF_89AB));
        fence(Ordering::SeqCst);
    }
    
}

fn flash_lock() {
    let flash = pac::FLASH;
    //flash.ctlr().modify(|w| w.set_lock(true));
    pac::FLASH.ctlr().modify(|w| {
        w.set_lock(true);
    });
    // Re-enter enhanced read mode for normal execution.
    flash_enter_enhanced_read();
}

fn flash_wait_ready() {
    let flash = pac::FLASH;
    while flash.statr().read().bsy() || flash.statr().read().wr_bsy() {}
}

fn flash_wait_ready_write() -> Result<(), StorageError> {
    let flash = pac::FLASH;
    loop {
        let status = flash.statr().read();
        if status.wr_bsy() == false {
            if status.wrprterr() {
                debug_mark((6, 0, 0).into()); // Next LED (red): write protection error.
                return Err(StorageError::WriteFailed);
            }
            return Ok(());
        }
    }
}

fn flash_clear_status() {
    let flash = pac::FLASH;
    // Clear EOP/WRPRTERR by writing 1s to the status bits.
    flash.statr().modify(|w| {
        w.set_eop(true);
        w.set_wrprterr(true);
    });
}

fn flash_erase_page(page_addr: usize) {
    let flash = pac::FLASH;
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_page_er(true));
    flash.addr().write(|w| w.set_far(page_addr as u32));
    fence(Ordering::SeqCst);
    flash.ctlr().modify(|w| w.set_strt(true));
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_page_er(false));
}

fn flash_erase_range(start: usize, len: usize) -> Result<(), StorageError> {
    let _clock_guard = FlashClockGuard::enter();
    let mut addr = start;
    let end = start
        .checked_add(len)
        .ok_or(StorageError::ProgramTooLarge)?;
    while addr < end {
        flash_erase_page(addr);
        flash_verify_erased_page(addr)?;
        addr = addr
            .checked_add(PAGE_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
    }
    Ok(())
}

fn flash_verify_erased_page(page_addr: usize) -> Result<(), StorageError> {
    let mut offset = 0usize;
    while offset < PAGE_SIZE_BYTES {
        let addr = page_addr
            .checked_add(offset)
            .ok_or(StorageError::ProgramTooLarge)?;
        let value = unsafe { core::ptr::read_volatile(addr as *const Word) };
        if value != 0xe339 {
            return Err(StorageError::InvalidHeader);
        }
        offset = offset
            .checked_add(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
    }
    Ok(())
}

fn debug_mark_word_bits(value: Word) {
    for bit in 0..16 {
        let set = (value >> (15 - bit)) & 1 == 1;
        if set {
            debug_mark((0, 32, 0).into());
        } else {
            debug_mark((32, 0, 0).into());
        }
    }
}

fn flash_program_word(addr: usize, value: Word) -> Result<(), StorageError> {
    flash_program_words(addr, &[value])
}

fn flash_program_words(start: usize, program: &[Word]) -> Result<(), StorageError> {
    let _clock_guard = FlashClockGuard::enter();
    if start % WORD_SIZE_BYTES != 0 {
        return Err(StorageError::UnalignedWrite);
    }

    let (storage_start, storage_end) = storage_bounds();
    let byte_len = program
        .len()
        .checked_mul(WORD_SIZE_BYTES)
        .ok_or(StorageError::ProgramTooLarge)?;
    let end = start
        .checked_add(byte_len)
        .ok_or(StorageError::ProgramTooLarge)?;
    if start < storage_start || end > storage_end {
        return Err(StorageError::ProgramTooLarge);
    }

    let flash = pac::FLASH;
    if flash.statr().read().enhance_mod_sta() == true {
        debug_mark((128, 0, 0).into());
        return Err(StorageError::WriteFailed);
    }

    let mut addr = start as u32;
    flash_wait_ready();
    flash_clear_status();
    flash_unlock();
    flash.ctlr().modify(|w| w.set_pg(true));
    for &word in program {
        // SAFETY: addr is aligned and inside the flash storage region.
        unsafe { write_volatile(addr as *mut u16, word) };
        fence(Ordering::SeqCst);
        flash_wait_ready_write()?;
        //flash_clear_status();
        let read_value = unsafe { core::ptr::read_volatile(addr as *const Word) }; // DEBUG
        if read_value != word {
            debug_mark((64, 64, 64).into()); // Next LED (red): write protection error.
            return Err(StorageError::WriteFailed);
        }
        addr = addr
            .checked_add(WORD_SIZE_BYTES as u32)
            .ok_or(StorageError::ProgramTooLarge)?;
    }
    flash.ctlr().modify(|w| w.set_pg(false));
    Ok(())
}

fn flash_program_header(storage_start: usize, program_words: usize) -> Result<(), StorageError> {
    let header_words = encode_header(program_words)?;
    flash_program_words(storage_start, &header_words)
}

fn flash_enter_enhanced_read() {
    let _clock_guard = FlashClockGuard::enter();
    let flash = pac::FLASH;
    flash.ctlr().modify(|w| w.set_enhancemode(true));
    for _ in 0..1_000_000 {
        if flash.statr().read().enhance_mod_sta() {
            return;
        }
    }
    //debug_mark((0, 64, 64).into()); // Next LED (): enter enhanced read timed out.
}

fn flash_exit_enhanced_read() {
    let _clock_guard = FlashClockGuard::enter();
    let flash = pac::FLASH;
    if !flash.statr().read().enhance_mod_sta() {
        return;
    }
    flash.ctlr().modify(|w| w.set_enhancemode(false));
    flash.ctlr().modify(|w| w.set_rsenact(true));
    for _ in 0..1_000_000 {
        if !flash.statr().read().enhance_mod_sta() {
            return;
        }
    }
    debug_mark((255, 0, 255).into()); // Next LED (): enhanced read exit timed out.
}

struct FlashClockGuard {
    // Original AHB prescaler so we can restore it after the flash op.
    prev_hpre: pac::rcc::vals::Hpre,
    restore: bool,
}

impl FlashClockGuard {
    fn enter() -> Self {
        let rcc = pac::RCC;
        let prev_hpre = rcc.cfgr0().read().hpre();
        // Only override the prescaler when the system is running at full speed.
        let restore = matches!(prev_hpre, pac::rcc::vals::Hpre::DIV1);
        if restore {
            // Divide HCLK by 2 to keep flash access clock <= 60 MHz for flash ops.
            rcc.cfgr0().modify(|w| w.set_hpre(pac::rcc::vals::Hpre::DIV2));
        }
        Self { prev_hpre, restore }
    }
}

impl Drop for FlashClockGuard {
    fn drop(&mut self) {
        if self.restore {
            let rcc = pac::RCC;
            // Restore the original prescaler after the flash operation completes.
            rcc.cfgr0().modify(|w| w.set_hpre(self.prev_hpre));
        }
    }
}
