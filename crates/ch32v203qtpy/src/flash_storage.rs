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
use core::ptr;

use ch32_hal::pac;
use light_machine::{Program, Word};
use pliot::{ProgramNumber, Storage, StorageError};

const PAGE_SIZE_BYTES: usize = 256;
const WORD_SIZE_BYTES: usize = 2;
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
    pub fn new() -> Self {
        Self { program_words: 0 }
    }

    fn write_program(&mut self, program: &[Word]) -> Result<(), StorageError> {
        // Treat the input slice as the complete program image.
        let byte_len = program
            .len()
            .checked_mul(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
        let (start, end) = storage_bounds();
        let capacity = end.checked_sub(start).ok_or(StorageError::ProgramTooLarge)?;
        if byte_len > capacity {
            return Err(StorageError::ProgramTooLarge);
        }

        let mut program_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            let erase_len = align_up(byte_len, PAGE_SIZE_BYTES);
            program_result = flash_erase_range(start, erase_len)
                .and_then(|_| flash_program_words(start, program));
            flash_lock();
        });
        program_result?;

        self.program_words = program.len();
        Ok(())
    }

    fn program_slice<'a>(&'a self) -> &'a [Word] {
        let (start, end) = storage_bounds();
        let max_len = end.saturating_sub(start) / WORD_SIZE_BYTES;
        let len = self.program_words.min(max_len);
        // SAFETY: The storage region is linker-defined, word-aligned, and lives for the
        // program lifetime, so it's valid to create a static slice over it.
        unsafe { core::slice::from_raw_parts(start as *const Word, len) }
    }
}

impl Default for FlashStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for FlashStorage {
    type L = FlashProgramLoader;

    /// `size` is in instruction count (words).
    fn get_program_loader(&mut self, size: u32) -> Result<Self::L, StorageError> {
        let size_words: usize = size.try_into().map_err(|_| StorageError::ProgramTooLarge)?;
        let (start, end) = storage_bounds();
        let capacity_words = end
            .checked_sub(start)
            .ok_or(StorageError::ProgramTooLarge)?
            / WORD_SIZE_BYTES;
        if size_words > capacity_words {
            return Err(StorageError::ProgramTooLarge);
        }

        // Erase is done up front so add_block can stream writes without
        // worrying about page alignment.
        let mut erase_result: Result<(), StorageError> = Ok(());
        critical_section::with(|_| {
            flash_unlock();
            let erase_len = align_up(size_words * WORD_SIZE_BYTES, PAGE_SIZE_BYTES);
            erase_result = flash_erase_range(start, erase_len);
            flash_lock();
        });
        erase_result?;

        self.program_words = size_words;
        Ok(FlashProgramLoader::new(start, size_words))
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
        loader.finish_load()
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
                let addr = self.program_start + next_word * WORD_SIZE_BYTES;
                program_result = flash_program_word(addr, *word);
                next_word += 1;
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

fn storage_bounds() -> (usize, usize) {
    // SAFETY: These are linker-provided symbols, so taking their addresses is safe and does
    // not require alignment. Alignment only matters when we later cast to `*const Word`, and
    // the storage region is defined on a flash boundary in the linker script.
    let start = unsafe { &__storage_start as *const u8 as usize };
    let end = unsafe { &__storage_end as *const u8 as usize };
    (start, end)
}

fn align_up(value: usize, align: usize) -> usize {
    let mask = align - 1;
    (value + mask) & !mask
}

fn flash_unlock() {
    let flash = pac::FLASH;
    // Enhanced read mode must be disabled before any erase/program sequence.
    flash_exit_enhanced_read();
    if flash.ctlr().read().lock() {
        flash.keyr().write(|w| w.set_keyr(FLASH_KEY1));
        flash.keyr().write(|w| w.set_keyr(FLASH_KEY2));
    }
}

fn flash_lock() {
    let flash = pac::FLASH;
    flash.ctlr().modify(|w| w.set_lock(true));
    // Re-enter enhanced read mode for normal execution.
    flash_enter_enhanced_read();
}

fn flash_wait_ready() {
    let flash = pac::FLASH;
    while flash.statr().read().bsy() || flash.statr().read().wr_bsy() {}
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
    flash.ctlr().modify(|w| w.set_per(true));
    flash.addr().write(|w| w.set_far(page_addr as u32));
    flash.ctlr().modify(|w| w.set_strt(true));
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_per(false));
}

fn flash_erase_range(start: usize, len: usize) -> Result<(), StorageError> {
    let _clock_guard = FlashClockGuard::enter();
    let mut addr = start;
    let end = start
        .checked_add(len)
        .ok_or(StorageError::ProgramTooLarge)?;
    while addr < end {
        flash_erase_page(addr);
        addr = addr
            .checked_add(PAGE_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
    }
    Ok(())
}

fn flash_program_word(addr: usize, value: Word) -> Result<(), StorageError> {
    // Flash programming requires halfword alignment.
    if addr % WORD_SIZE_BYTES != 0 {
        return Err(StorageError::UnalignedWrite);
    }
    let (start, end) = storage_bounds();
    // Reject writes outside the reserved flash storage window.
    if addr < start || addr >= end {
        return Err(StorageError::ProgramTooLarge);
    }
    let flash = pac::FLASH;
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_pg(true));
    // SAFETY: `addr` is validated to be aligned and within the flash storage region.
    unsafe { ptr::write_volatile(addr as *mut Word, value) };
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_pg(false));
    Ok(())
}

fn flash_program_words(start: usize, program: &[Word]) -> Result<(), StorageError> {
    let _clock_guard = FlashClockGuard::enter();
    let mut addr = start;
    for word in program {
        flash_program_word(addr, *word)?;
        addr = addr
            .checked_add(WORD_SIZE_BYTES)
            .ok_or(StorageError::ProgramTooLarge)?;
    }
    Ok(())
}

fn flash_enter_enhanced_read() {
    let flash = pac::FLASH;
    flash.ctlr().modify(|w| w.set_enhancemode(true));
    while !flash.statr().read().enhance_mod_sta() {}
}

fn flash_exit_enhanced_read() {
    let flash = pac::FLASH;
    flash.ctlr().modify(|w| w.set_enhancemode(false));
    flash.ctlr().modify(|w| w.set_rsenact(true));
    while flash.statr().read().enhance_mod_sta() {}
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
