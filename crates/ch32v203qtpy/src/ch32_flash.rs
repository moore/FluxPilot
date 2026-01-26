use core::ptr::write_volatile;
use core::sync::atomic::{fence, Ordering};

use ch32_hal::pac;
use embedded_storage::nor_flash::{
    check_erase, check_read, check_write, ErrorType, NorFlash, NorFlashError, NorFlashErrorKind,
    ReadNorFlash,
};

const PAGE_SIZE_BYTES: usize = 4096;
const WORD_SIZE_BYTES: usize = 2;
const FLASH_KEY1: u32 = 0x4567_0123;
const FLASH_KEY2: u32 = 0xCDEF_89AB;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FlashError {
    NotAligned,
    OutOfBounds,
    WriteFailed,
}

impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            FlashError::NotAligned => NorFlashErrorKind::NotAligned,
            FlashError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            FlashError::WriteFailed => NorFlashErrorKind::Other,
        }
    }
}

impl From<NorFlashErrorKind> for FlashError {
    fn from(kind: NorFlashErrorKind) -> Self {
        match kind {
            NorFlashErrorKind::NotAligned => FlashError::NotAligned,
            NorFlashErrorKind::OutOfBounds => FlashError::OutOfBounds,
            NorFlashErrorKind::Other => FlashError::WriteFailed,
        }
    }
}

pub struct Ch32Flash {
    base: usize,
    size: usize,
}

impl Ch32Flash {
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }

    fn addr(&self, offset: u32) -> Result<usize, FlashError> {
        let offset = usize::try_from(offset).map_err(|_| FlashError::OutOfBounds)?;
        let addr = self.base.checked_add(offset).ok_or(FlashError::OutOfBounds)?;
        let end = self.base.checked_add(self.size).ok_or(FlashError::OutOfBounds)?;

        if addr < self.base || addr >= end {
            return Err(FlashError::OutOfBounds);
        }
        Ok(addr)
    }
}

impl ErrorType for Ch32Flash {
    type Error = FlashError;
}

impl ReadNorFlash for Ch32Flash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        check_read(self, offset, bytes.len()).map_err(FlashError::from)?;
        let addr = self.addr(offset)?;
        // SAFETY: reading from memory-mapped flash.
        let src = unsafe { core::slice::from_raw_parts(addr as *const u8, bytes.len()) };
        bytes.copy_from_slice(src);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.size
    }
}

impl NorFlash for Ch32Flash {
    const WRITE_SIZE: usize = WORD_SIZE_BYTES;
    const ERASE_SIZE: usize = PAGE_SIZE_BYTES;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        check_erase(self, from, to).map_err(FlashError::from)?;
        critical_section::with(|_| {
            let _clock_guard = FlashClockGuard::enter();
            flash_unlock();
            let mut addr = self.addr(from).map_err(|_| FlashError::OutOfBounds)?;
            let end = self
                .base
                .checked_add(usize::try_from(to).map_err(|_| FlashError::OutOfBounds)?)
                .ok_or(FlashError::OutOfBounds)?;
            while addr < end {
                flash_erase_page(addr);
                addr = addr
                    .checked_add(PAGE_SIZE_BYTES)
                    .ok_or(FlashError::OutOfBounds)?;
            }
            flash_lock();
            Ok(())
        })
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        check_write(self, offset, bytes.len()).map_err(FlashError::from)?;
        if bytes.len() % WORD_SIZE_BYTES != 0 {
            return Err(FlashError::NotAligned);
        }
        critical_section::with(|_| {
            let _clock_guard = FlashClockGuard::enter();
            flash_unlock();
            flash_program_words(self.addr(offset)?, bytes)?;
            flash_lock();
            Ok(())
        })
    }
}

fn flash_unlock() {
    let _clock_guard = FlashClockGuard::enter();
    // Enhanced read mode must be disabled before any erase/program sequence.
    flash_exit_enhanced_read();
    if pac::FLASH.ctlr().read().lock() {
        pac::FLASH.keyr().write(|w| w.set_keyr(FLASH_KEY1));
        fence(Ordering::SeqCst);
        pac::FLASH.keyr().write(|w| w.set_keyr(FLASH_KEY2));
        fence(Ordering::SeqCst);
    }
    if pac::FLASH.ctlr().read().flock() {
        pac::FLASH.modekeyr().write(|w| w.set_modekeyr(FLASH_KEY1));
        fence(Ordering::SeqCst);
        pac::FLASH.modekeyr().write(|w| w.set_modekeyr(FLASH_KEY2));
        fence(Ordering::SeqCst);
    }
}

fn flash_lock() {
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

fn flash_wait_ready_write() -> Result<(), FlashError> {
    let flash = pac::FLASH;
    loop {
        let status = flash.statr().read();
        if !status.wr_bsy() {
            if status.wrprterr() {
                return Err(FlashError::WriteFailed);
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

fn flash_program_words(addr: usize, bytes: &[u8]) -> Result<(), FlashError> {
    if addr % WORD_SIZE_BYTES != 0 {
        return Err(FlashError::NotAligned);
    }
    let flash = pac::FLASH;
    if flash.statr().read().enhance_mod_sta() {
        return Err(FlashError::WriteFailed);
    }

    let mut addr = addr as u32;
    flash_wait_ready();
    flash_clear_status();
    flash.ctlr().modify(|w| w.set_pg(true));
    for chunk in bytes.chunks_exact(WORD_SIZE_BYTES) {
        let word = u16::from_le_bytes([chunk[0], chunk[1]]);
        // SAFETY: addr is aligned and inside the flash storage region.
        unsafe { write_volatile(addr as *mut u16, word) };
        fence(Ordering::SeqCst);
        flash_wait_ready_write()?;
        let read_value = unsafe { core::ptr::read_volatile(addr as *const u16) };
        if read_value != word {
            return Err(FlashError::WriteFailed);
        }
        addr = addr
            .checked_add(WORD_SIZE_BYTES as u32)
            .ok_or(FlashError::OutOfBounds)?;
    }
    flash.ctlr().modify(|w| w.set_pg(false));
    Ok(())
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
