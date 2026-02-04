
# FluxPiolet Roadmap

## Pahse 1

 1. ~~Implment call API in JS.~~
 2. ~~Update event loop on board side to speak wire protocol.~~
 3. ~~Update UI to make RPCs to update color.~~
 4. ~~Implment js API to get default program.~~
 5. ~~Implment js API to load program.~~
 6. ~~Implment program loding on board.~~
 7. ~~Implment js program builder API.~~
 8. ~~Implment program storage on board.~~

## Phase 2

 1. ~~Debug why large programs crash the fermware~~
 2. ~~Store UI state on flash and reload to UI on usb connect.~~
 3. ~~Move to to u32 for stack instead of u16~~
 4. ~~Allow common functions in shared static to reduce duplcation across machines.~~
 5. ~~Combine stack and heap in to single allocation.~~
    - fix the overuse of unsafe.
 
## Phase 3

 1. Implment i2c routing

## Phase 4

 1. Implment LED configuration wizerd.

## Phase 5
 
 1. Polish UI
 2. Pay desiner to make nice lookin.

## Release v1

 0. Review and clean up code base.
 1. Create minimal docs.
 2. Make web site.
 3. Clean up GitHub and Readmes
 4. Anounce on Mastodon.
 5. Create YouTube demo.

## Later

- Implment emulator in webapp.
- Add a watchdog reset in firmware to recover from hangs (flash ops timeout).
- Implment LED configuration wizerd.
- Implment i2c routing.
- Implment Adfruit encoder board i2c parcer.
- Implment custom ws2812 driver that dose not do so 
  much copying and makes sure the data is sent out with DMA.
- Investigate WS2812 timing when SYSCLK is lowered (SPI frequency/prescaler issues).
- Investigate using the CH32V203 hardware CRC peripheral for flash header checks.
- Add a note about .data LMA overflow: main FLASH stays 32K, consider a custom linker script to place .data load image in a `.colddata`/FLASH1 region (e.g. `AT>FLASH1`) to avoid overlapping `.coldtext`.
- Cosider requiring an explisit command to format the flash instead of doing it if we get invlid headder.
