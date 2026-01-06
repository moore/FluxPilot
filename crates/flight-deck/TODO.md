
# FluxPiolet Roadmap

## Next Steps

 1. ~~Implment call API in JS.~~
 2. ~~Update event loop on board side to speak wire protocol.~~
 3. ~~Update UI to make RPCs to update color.~~
 4. ~~Implment js API to get default program.~~
 5. ~~Implment js API to load program.~~
 6. ~~Implment program loding on board.~~
 7. ~~Implment js program builder API.~~
 8. Implment program storage on board.

## Later

- Implment basic UI for custome program.
- Add a watchdog reset in firmware to recover from hangs (flash ops timeout).
- Implment LED configuration wizerd.
- Implment i2c routing.
- Implment Adfruit encoder board i2c parcer.
- Implment custom ws2812 driver that dose not do so 
  much copying and makes sure the data is sent out with DMA.
- Investigate using the CH32V203 hardware CRC peripheral for flash header checks.
- Add a note about .data LMA overflow: main FLASH stays 32K, consider a custom linker script to place .data load image in a `.colddata`/FLASH1 region (e.g. `AT>FLASH1`) to avoid overlapping `.coldtext`.
- Cosider requiring an explisit command to format the flash instead of doing it if we get invlid headder.
