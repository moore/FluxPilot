
import init, { FlightDeck, compile_program } from "/pkg/flight_deck.js";
import AsyncQueue from "/async_queue.js";

const statusEls = Array.from(document.querySelectorAll('[data-status-message]')).filter(Boolean);
const connectBtn = document.getElementById('connect');
const statusPillEl = document.getElementById('status-pill');
const loadProgramBtn = document.getElementById('load-program');
const brightnessEl = document.getElementById('brightness-slider');
const brightnessValueEl = document.getElementById('brightness-value');
const editorEl = document.getElementById('program-editor');
let writer = null;
const SEND_QUEUE_KEY = "__toSendQueue__";
const DECK_KEY = "__flightDeck__";
const HANDLERS_BOUND_KEY = "__flightDeckHandlersBound__";
const UI_STATE_SERIALIZE_KEY = "__serializeUiState";
const UI_STATE_RESTORE_KEY = "__restoreUiState";
const GLOBAL_BRIGHTNESS_FUNCTION = 4;
const CONTROL_STATIC_PREFIX = "init_";
const CONTROL_STATIC_BLOCK = "control_statics";
let usbReadActive = false;
let pendingRequestId = null;
let pendingStartTime = 0;
let pendingTimer = null;
let pendingCalls = new Map();
let connectInFlight = false;
let uiStateFetch = null;
let pendingUiStateBytes = null;
let incomingFrame = [];
let i2cDeviceFetch = null;

function clampWord(value) {
    const numberValue = Number(value);
    if (!Number.isFinite(numberValue)) {
        return 0;
    }
    const rounded = Math.round(numberValue);
    return Math.max(0, Math.min(65535, rounded));
}

function parseHexColor(value) {
    if (!value) {
        return null;
    }
    const normalized = value.startsWith('#') ? value.slice(1) : value;
    const hex = normalized.length === 8 ? normalized.slice(0, 6) : normalized;
    if (hex.length !== 6) {
        return null;
    }
    const r = Number.parseInt(hex.slice(0, 2), 16);
    const g = Number.parseInt(hex.slice(2, 4), 16);
    const b = Number.parseInt(hex.slice(4, 6), 16);
    if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) {
        return null;
    }
    return { r, g, b };
}

function getUiStateSerializer() {
    const fn = globalThis[UI_STATE_SERIALIZE_KEY];
    return typeof fn === 'function' ? fn : null;
}

function getUiStateRestorer() {
    const fn = globalThis[UI_STATE_RESTORE_KEY];
    return typeof fn === 'function' ? fn : null;
}

async function gzipBytes(bytes) {
    if (!('CompressionStream' in globalThis)) {
        throw new Error('CompressionStream not supported.');
    }
    const compressed = await new Response(
        new Blob([bytes]).stream().pipeThrough(new CompressionStream('gzip'))
    ).arrayBuffer();
    return new Uint8Array(compressed);
}

async function gunzipBytes(bytes) {
    if (!('DecompressionStream' in globalThis)) {
        throw new Error('DecompressionStream not supported.');
    }
    const decompressed = await new Response(
        new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'))
    ).arrayBuffer();
    return new Uint8Array(decompressed);
}

async function buildCompressedUiState() {
    const serialize = getUiStateSerializer();
    if (!serialize) {
        return new Uint8Array();
    }
    const raw = serialize();
    if (!(raw instanceof Uint8Array)) {
        return new Uint8Array();
    }
    if (!raw.length) {
        return raw;
    }
    console.log("uncompressed UI len", raw.length);
    const result = await gzipBytes(raw);
    console.log("compressed size", result.length);
    return result;
}

async function applyCompressedUiState(bytes) {
    if (!bytes || !bytes.length) {
        return;
    }
    const restore = getUiStateRestorer();
    if (!restore) {
        pendingUiStateBytes = bytes;
        setTimeout(() => {
            if (pendingUiStateBytes) {
                void applyCompressedUiState(pendingUiStateBytes);
                pendingUiStateBytes = null;
            }
        }, 250);
        return;
    }
    try {
        const decompressed = await gunzipBytes(bytes);
        restore(decompressed, { replace: true });
    } catch (err) {
        console.error('UI state restore failed:', err);
        setStatus('Failed to restore UI state from device.');
    }
}

export function notifyUiReady() {
    if (pendingUiStateBytes) {
        const bytes = pendingUiStateBytes;
        pendingUiStateBytes = null;
        void applyCompressedUiState(bytes);
    }
}

function startUiStateFetch() {
    const deck = globalThis[DECK_KEY];
    if (!deck) {
        return;
    }
    uiStateFetch = {
        totalSize: null,
        received: 0,
        nextBlock: 0,
        chunks: [],
        inFlight: false,
    };
    requestNextUiStateBlock();
}

function requestNextUiStateBlock() {
    const deck = globalThis[DECK_KEY];
    if (!deck || !uiStateFetch || uiStateFetch.inFlight) {
        return;
    }
    uiStateFetch.inFlight = true;
    try {
        let request_id = deck.read_ui_state_block(uiStateFetch.nextBlock);
    } catch (err) {
        uiStateFetch = null;
    }
}

function scheduleNextUiStateBlock() {
    setTimeout(() => {
        requestNextUiStateBlock();
    }, 0);
}

function getTrackControlStatics(track) {
    const trackMachine = track.querySelector('fd-track-machine');
    const controlsRoot = trackMachine?.shadowRoot?.querySelector('.track-controls');
    if (!controlsRoot) {
        return [];
    }
    const statics = new Map();
    const controls = controlsRoot.querySelectorAll('fd-range-control, fd-color-picker');
    controls.forEach((control) => {
        const localsRaw = control.getAttribute('data-locals');
        if (!localsRaw) {
            return;
        }
        const locals = localsRaw.split(',').map((entry) => entry.trim()).filter(Boolean);
        if (!locals.length) {
            return;
        }
        const tag = control.tagName.toLowerCase();
        if (tag === 'fd-range-control') {
            const value = clampWord(control.getAttribute('value') ?? control.value);
            locals.forEach((local) => {
                statics.set(local, value);
            });
            return;
        }
        if (tag === 'fd-color-picker') {
            const rgb = parseHexColor(control.getAttribute('value') ?? control.value);
            if (!rgb) {
                return;
            }
            const values = [rgb.r, rgb.g, rgb.b];
            locals.forEach((local, index) => {
                if (index >= values.length) {
                    return;
                }
                statics.set(local, clampWord(values[index]));
            });
        }
    });
    return Array.from(statics.entries()).map(([local, value]) => ({
        label: `${CONTROL_STATIC_PREFIX}${local}`,
        value,
    }));
}

function stripControlStaticsBlock(source) {
    const lines = source.split(/\r?\n/);
    const header = `.data ${CONTROL_STATIC_BLOCK}`;
    let startIndex = -1;
    for (let i = 0; i < lines.length; i++) {
        if (lines[i].trim() === header) {
            startIndex = i;
            break;
        }
    }
    if (startIndex === -1) {
        return source;
    }
    let endIndex = -1;
    for (let i = startIndex + 1; i < lines.length; i++) {
        if (lines[i].trim() === '.end') {
            endIndex = i;
            break;
        }
    }
    if (endIndex === -1) {
        return source;
    }
    lines.splice(startIndex, endIndex - startIndex + 1);
    return lines.join('\n');
}

function buildControlStaticsBlock(statics) {
    if (!statics.length) {
        return '';
    }
    const lines = [`.data ${CONTROL_STATIC_BLOCK}`];
    statics.forEach(({ label, value }) => {
        lines.push(`    ${label}:`);
        lines.push(`    .word ${value}`);
    });
    lines.push('.end');
    return lines.join('\n');
}

function injectControlStatics(source, statics) {
    if (!statics.length) {
        return source;
    }
    const cleaned = stripControlStaticsBlock(source);
    const block = buildControlStaticsBlock(statics);
    if (!block) {
        return cleaned;
    }
    const lines = cleaned.split(/\r?\n/);
    let insertIndex = -1;
    for (let i = 0; i < lines.length; i++) {
        const trimmed = lines[i].trim();
        if (!trimmed) {
            continue;
        }
        if (trimmed.startsWith('.machine')) {
            continue;
        }
        if (trimmed.startsWith('.local')) {
            continue;
        }
        if (
            trimmed.startsWith('.func') ||
            trimmed.startsWith('.func_decl') ||
            trimmed.startsWith('.data') ||
            trimmed === '.end'
        ) {
            insertIndex = i;
            break;
        }
    }
    if (insertIndex === -1) {
        return `${cleaned.trimEnd()}\n${block}\n`;
    }
    lines.splice(insertIndex, 0, ...block.split('\n'));
    return lines.join('\n');
}

class DeckReceiveHandler {
    onReturn(requestId, result) {
        resolvePending(requestId);
    }

    onNotification(machineIndex, functionIndex, result) {
        console.log("notification", machineIndex, functionIndex, Array.from(result));
    }

    onError(hasRequestId, requestId, errorCode, errorString) {
        console.warn("error", { hasRequestId, requestId, errorCode, errorString});
    }

    onUiStateBlock(requestId, totalSize, blockNumber, block) {
        if (!uiStateFetch) {
            return;
        }
        uiStateFetch.inFlight = false;
        if (uiStateFetch.totalSize === null) {
            uiStateFetch.totalSize = totalSize;
        }
        if (!totalSize) {
            uiStateFetch = null;
            return;
        }
        if (blockNumber !== uiStateFetch.nextBlock) {
            console.warn('Unexpected UI state block', { blockNumber, expected: uiStateFetch.nextBlock });
            uiStateFetch = null;
            return;
        }
        const chunk = block instanceof Uint8Array ? block.slice() : new Uint8Array(block);
        uiStateFetch.chunks.push(chunk);
        uiStateFetch.received += chunk.length;
        uiStateFetch.nextBlock += 1;
        if (uiStateFetch.received >= uiStateFetch.totalSize) {
            const total = uiStateFetch.totalSize;
            const combined = new Uint8Array(total);
            let offset = 0;
            uiStateFetch.chunks.forEach((part) => {
                const remaining = total - offset;
                if (remaining <= 0) {
                    return;
                }
                const slice = part.length > remaining ? part.slice(0, remaining) : part;
                combined.set(slice, offset);
                offset += slice.length;
            });
            uiStateFetch = null;
            void applyCompressedUiState(combined);
        } else {
            scheduleNextUiStateBlock();
        }
    }

    onI2cDevices(requestId, totalCount, devices) {
        const page = Array.from(devices);
        if (!i2cDeviceFetch) {
            i2cDeviceFetch = {
                totalCount,
                nextOffset: 0,
                devices: [],
            };
        }
        i2cDeviceFetch.totalCount = totalCount;
        i2cDeviceFetch.devices.push(...page);
        i2cDeviceFetch.nextOffset = i2cDeviceFetch.devices.length;

        console.log("I2C devices page", {
            requestId,
            totalCount,
            page,
            collected: [...i2cDeviceFetch.devices],
        });

        if (i2cDeviceFetch.nextOffset < totalCount && page.length > 0) {
            requestI2cDevicesPage(i2cDeviceFetch.nextOffset);
            return;
        }

        console.log("I2C devices discovered", [...i2cDeviceFetch.devices]);
    }
}

const receiveHandler = new DeckReceiveHandler();

export async function initDeck() {
    await init();
    globalThis[SEND_QUEUE_KEY] ??= new AsyncQueue();
    globalThis[DECK_KEY] ??= new FlightDeck();
    if (editorEl && !editorEl.value.trim()) {
        editorEl.value = `
.machine main locals 5 functions 5

.func init index 0
    LOAD_STATIC init_red
    LSTORE 0
    LOAD_STATIC init_green
    LSTORE 1
    LOAD_STATIC init_blue
    LSTORE 2
    LOAD_STATIC init_brightness
    LSTORE 3
    PUSH 0
    LSTORE 4
    EXIT
.end

.func start_frame index 1
    LSTORE 4
    EXIT
.end

.func set_rgb index 3
    LSTORE 2
    LSTORE 1
    LSTORE 0
    EXIT
.end

.func set_brightness index 4
    LSTORE 3
    EXIT
.end

.func get_rgb index 2
    LLOAD 4
    DUP
    PUSH 2000
    MOD
    SWAP
    PUSH 2000
    DIV
    PUSH 2
    MOD
    PUSH 0
    BREQ even
    PUSH 2000
    SWAP
    SUB
    even:
    DUP
    MUL
    PUSH 42
    MUL
    PUSH 4000
    DIV
    PUSH 18000
    ADD
    DUP
    LLOAD 0
    MUL
    PUSH 60000
    DIV
    LLOAD 3
    MUL
    PUSH 100
    DIV
    DUP
    LLOAD 1
    MUL
    PUSH 60000
    DIV
    LLOAD 3
    MUL
    PUSH 100
    DIV
    LLOAD 2
    MUL
    PUSH 60000
    DIV
    LLOAD 3
    MUL
    PUSH 100
    DIV
    EXIT
.end

.data control_statics
    init_red:
    .word 8
    init_green:
    .word 16
    init_blue:
    .word 32
    init_brightness:
    .word 100
.end

.end
`;
    }
    return globalThis[DECK_KEY];
}

export function send(temp_buffer) {
    // Note: The `message` is a buffer that get's reused.
    // it will be overwritten in queue if we don't clone it.
    const message = new Uint8Array(temp_buffer);
    globalThis[SEND_QUEUE_KEY].enqueue(message);
}


export async function consumeQueue() {
    while (true) {
        let message = await globalThis[SEND_QUEUE_KEY].dequeue();
        await sendMessage(message);
    }
} 

export function callMachineFunction(machineIndex, functionIndex, args) {

    let deck = globalThis[DECK_KEY];
    if (!deck) {
        setStatus('Deck not initialized yet.');
        return null;
    }
    if (pendingRequestId !== null) {
        const key = `${machineIndex}:${functionIndex}`;
        pendingCalls.delete(key);
        pendingCalls.set(key, { machineIndex, functionIndex, args });
        return null;
    }
    try {
        console.log("calling function ", machineIndex, functionIndex, args);
        pendingStartTime = performance.now();
        const requestId = deck.call(machineIndex, functionIndex, args);
        if (requestId !== undefined && requestId !== null) {
            pendingRequestId = requestId;
            setPendingTimeout();
        }
        return requestId;
    } catch (err) {
        setStatus('Call failed: ' + (err.message || err));
        return null;
    }
}

function setStatus(msg) {
    statusEls.forEach((el) => {
        if (!el) {
            return;
        }
        el.textContent = msg;
    });
}

function setConnectionState(isConnected) {
    if (!statusPillEl) {
        return;
    }
    statusPillEl.setAttribute('state', isConnected ? 'connected' : 'disconnected');
}

function buildProgramSourceFromTracks() {
    const trackList = document.getElementById('track-list');
    if (!trackList) {
        return { source: '', error: 'Track list not available.', isEmpty: true };
    }
    const tracks = Array.from(trackList.querySelectorAll('fd-track'));
    const sources = [];
    const missing = [];

    tracks.forEach((track, index) => {
        const machineId = track.dataset.machineId || '';
        const machineAssembly = track.dataset.machineAssembly || '';
        const machineSource = track.dataset.machineSource || '';
        const isEmpty = !machineId && !machineAssembly;
        if (isEmpty) {
            return;
        }
        let rawSource = '';
        if (machineSource === 'editor' && editorEl?.value?.trim()) {
            rawSource = editorEl.value;
        } else if (machineAssembly.trim()) {
            rawSource = machineAssembly;
        }
        if (rawSource) {
            const statics = getTrackControlStatics(track);
            const sourceWithStatics = injectControlStatics(rawSource, statics);
            if (machineSource === 'editor' && editorEl && editorEl.value !== sourceWithStatics) {
                editorEl.value = sourceWithStatics;
            }
            if (machineAssembly !== sourceWithStatics) {
                track.dataset.machineAssembly = sourceWithStatics;
            }
            sources.push(sourceWithStatics);
            return;
        }
        missing.push(machineId || `track ${index + 1}`);
    });

    if (missing.length) {
        return {
            source: '',
            error: `Missing assembly for ${missing.join(', ')}.`,
            isEmpty: false,
        };
    }
    if (!sources.length) {
        return { source: '', error: 'No machines selected in tracks.', isEmpty: true };
    }
    return { source: sources.join('\n\n'), isEmpty: false };
}

function enableControls() { 
    connectBtn.disabled = true;
    loadProgramBtn.disabled = false;
    brightnessEl.disabled = false;
}

function disableControls() {
    connectBtn.disabled = false;
    loadProgramBtn.disabled = true;
    brightnessEl.disabled = true;
}

function handleDisconnect(message) {
    writer = null;
    uiStateFetch = null;
    i2cDeviceFetch = null;
    setConnectionState(false);
    disableControls();
    if (message) {
        setStatus(message);
    }
}

function requestI2cDevicesPage(offset) {
    const deck = globalThis[DECK_KEY];
    if (!deck) {
        return;
    }
    try {
        deck.get_i2c_devices(offset);
    } catch (err) {
        console.error("get_i2c_devices failed:", err);
    }
}

function startI2cDeviceFetch() {
    i2cDeviceFetch = {
        totalCount: null,
        nextOffset: 0,
        devices: [],
    };
    requestI2cDevicesPage(0);
}

function resolvePending(requestId) {
    if (pendingRequestId === null || pendingRequestId !== requestId) {
        return false;
    }
    const now = performance.now();
    const runtime = now - pendingStartTime;
    console.log("request took", runtime);
    pendingRequestId = null;
    if (pendingTimer) {
        clearTimeout(pendingTimer);
        pendingTimer = null;
    }
    if (pendingCalls.size) {
        const [key, next] = pendingCalls.entries().next().value;
        pendingCalls.delete(key);
        callMachineFunction(next.machineIndex, next.functionIndex, next.args);
    }
    return true;
}

function setPendingTimeout() {
    if (pendingTimer) {
        clearTimeout(pendingTimer);
    }
    pendingTimer = setTimeout(() => {
        pendingTimer = null;
        if (pendingRequestId !== null) {
            pendingRequestId = null;
            if (pendingCalls.size) {
                const [key, next] = pendingCalls.entries().next().value;
                pendingCalls.delete(key);
                callMachineFunction(next.machineIndex, next.functionIndex, next.args);
            }
        }
    }, 200);
}

function getActiveTracks() {
    const trackList = document.getElementById('track-list');
    if (!trackList) {
        return [];
    }
    return Array.from(trackList.querySelectorAll('fd-track')).filter((track) => {
        const machineId = track.dataset.machineId || '';
        const machineAssembly = track.dataset.machineAssembly || '';
        return Boolean(machineId || machineAssembly);
    });
}

function applyGlobalBrightness(value) {
    const tracks = getActiveTracks();
    if (!tracks.length) {
        return;
    }
    for (const [key, call] of pendingCalls.entries()) {
        if (call.functionIndex === GLOBAL_BRIGHTNESS_FUNCTION) {
            pendingCalls.delete(key);
        }
    }
    tracks.forEach((_track, index) => {
        callMachineFunction(index, GLOBAL_BRIGHTNESS_FUNCTION, [value]);
    });
}

export async function connect() {
    setStatus('Connecting...');
    if (!('usb' in navigator)) {
        setStatus('WebUSB not available in this browser.');
        return;
    }
    if (!window.isSecureContext) {
        setStatus('WebUSB requires HTTPS or localhost.');
        return;
    }
    try {
        setStatus('Requesting USB device...');
        // WCH CH32V203 and common USB chip vendor IDs
        const filters = [
            { vendorId: 0xc0de },
            { vendorId: 0x1A86 }, // WinChipHead (WCH) - CH32V203, CH340/CH341
            { vendorId: 0x4348 }, // WCH alternative vendor ID
            { vendorId: 0x10C4 }, // Silicon Labs CP210x
            { vendorId: 0x0403 }, // FTDI
            { vendorId: 0x067B }, // Prolific PL2303
            { vendorId: 0x2341 }, // Arduino
            { vendorId: 0x239A }, // Adafruit
            { vendorId: 0x1209 }, // Generic
            { vendorId: 0x2E8A }, // Rpi RP2035
        ];
        
        const device = await navigator.usb.requestDevice({ filters });
        await connectUsbDevice(device);
    } catch (err) {
        console.error('Connect error:', err);
        setConnectionState(false);
        disableControls();
        setStatus('Connection failed: ' + (err.message || err));
    }
}

async function openUsbDevice(device) {
    if (device.opened) {
        return;
    }
    for (let attempt = 0; attempt < 3; attempt++) {
        try {
            await device.open();
            return;
        } catch (err) {
            if (err?.name === 'InvalidStateError' && attempt < 2) {
                await new Promise((resolve) => setTimeout(resolve, 120));
                continue;
            }
            throw err;
        }
    }
}

async function connectUsbDevice(device) {
    if (!device) {
        return;
    }
    if (connectInFlight) {
        return;
    }
    connectInFlight = true;
    try {
        await openUsbDevice(device);

        // Try to select the first available configuration
        if (device.configuration === null) {
            await device.selectConfiguration(1);
        }

        const ifaceInfo = findUsbInterface(device);
        if (!ifaceInfo) {
            throw new Error('No suitable USB interface with bulk IN/OUT endpoints');
        }
        const { interfaceNumber, alternateSetting, inEndpoint, outEndpoint } = ifaceInfo;

        await device.claimInterface(interfaceNumber);
        if (alternateSetting !== 0) {
            await device.selectAlternateInterface(interfaceNumber, alternateSetting);
        }

        writer = { device, type: 'usb', inEndpoint, outEndpoint };
        startUsbReceiveLoop(device);
        enableControls();
        setConnectionState(true);
        setStatus('Connected via WebUSB.');
        startUiStateFetch();
        startI2cDeviceFetch();
    } finally {
        connectInFlight = false;
    }
}

export async function autoConnect() {
    if (!('usb' in navigator) || !window.isSecureContext) {
        return;
    }
    try {
        const devices = await navigator.usb.getDevices();
        if (!devices.length) {
            return;
        }
        await connectUsbDevice(devices[0]);
    } catch (err) {
        console.error('Auto-connect error:', err);
    } finally {
        connectInFlight = false;
    }
}

async function sendMessage(message) {
    if (!writer) { 
        console.log("not connected in sendMessage");
        setStatus('Device not connected.'); 
        return;
    }
    try {
        if (writer.type === 'usb') {
            await writer.device.transferOut(writer.outEndpoint, message);
        } else {
            console.log("no USB in sendMessage!")
        }
        setStatus(`Sent ${message.length} bytes`);
    } catch (err) {
        console.error('Write error:', err);
        handleDisconnect('Send failed: ' + (err.message || err));
    }
}

function findUsbInterface(device) {
    if (!device.configuration) {
        return null;
    }
    for (const iface of device.configuration.interfaces) {
        for (const alt of iface.alternates) {
            const inEp = alt.endpoints.find(
                (ep) => ep.direction === 'in' && ep.type === 'bulk'
            );
            const outEp = alt.endpoints.find(
                (ep) => ep.direction === 'out' && ep.type === 'bulk'
            );
            if (inEp && outEp) {
                return {
                    interfaceNumber: iface.interfaceNumber,
                    alternateSetting: alt.alternateSetting,
                    inEndpoint: inEp.endpointNumber,
                    outEndpoint: outEp.endpointNumber,
                };
            }
        }
    }
    return null;
}

async function startUsbReceiveLoop(device) {
    if (usbReadActive) {
        return;
    }
    usbReadActive = true;
    while (writer && writer.type === 'usb' && writer.device === device) {
        try {
            const result = await device.transferIn(writer.inEndpoint, 64);
            if (result.status === 'ok' && result.data) {
                const data = new Uint8Array(
                    result.data.buffer,
                    result.data.byteOffset,
                    result.data.byteLength
                );
                let deck = globalThis[DECK_KEY];
                if (!deck) {
                    setStatus('Deck not initialized yet.');
                    continue;
                }
                for (const byte of data) {
                    incomingFrame.push(byte);
                    if (byte !== 0) {
                        continue;
                    }
                    const frame = new Uint8Array(incomingFrame);
                    incomingFrame = [];
                    try {
                        deck.receive(frame, receiveHandler);
                    } catch (err) {
                        console.error('Receive error:', err);
                    }
                }
            } else {
                console.warn('USB read status:', result.status);
            }
        } catch (err) {
            console.error('USB read error:', err);
            handleDisconnect('Device disconnected.');
            break;
        }
    }
    usbReadActive = false;
}

if (!globalThis[HANDLERS_BOUND_KEY]) {
    globalThis[HANDLERS_BOUND_KEY] = true;
    connectBtn?.addEventListener('click', connect);
    loadProgramBtn?.addEventListener('click', async () => {
        let deck = globalThis[DECK_KEY];
        if (!deck) {
            setStatus('Deck not initialized yet.');
            return;
        }
    if (!editorEl) {
        const { source, error } = buildProgramSourceFromTracks();
        if (error) {
            setStatus(error);
            return;
        }
        const programBuffer = new Uint16Array(4096);
        try {
            const descriptor = compile_program(source, programBuffer);
            console.log("program length: ", descriptor.length);

            const uiStateBytes = await buildCompressedUiState();
            deck.load_program(programBuffer, descriptor.length, uiStateBytes);
            setStatus(`Loaded program (${descriptor.length} words)`);
        } catch (err) {
            console.error('Load program error:', err);
            setStatus('Load program failed: ' + (err.message || err));
        }
        return;
    }
    const { source, error, isEmpty } = buildProgramSourceFromTracks();
    const programSource = source || editorEl.value;
    if (error && !isEmpty) {
        setStatus(error);
        return;
    }
    if (error && !programSource.trim()) {
        setStatus(error);
        return;
    }
    const programBuffer = new Uint16Array(4096);
    try {
        const descriptor = compile_program(programSource, programBuffer);
        console.log("program length: ", descriptor.length);
        const uiStateBytes = await buildCompressedUiState();
        deck.load_program(programBuffer, descriptor.length, uiStateBytes);
        setStatus(`Loaded program (${descriptor.length} words)`);
        } catch (err) {
            console.error('Load program error:', err);
            setStatus('Load program failed: ' + (err.message || err));
        }
    });

    brightnessEl?.addEventListener('input', () => {
        const value = Number(brightnessEl.value);
        const clamped = Math.max(0, Math.min(100, value));
        brightnessValueEl.textContent = `${clamped}%`;
        applyGlobalBrightness(clamped);
    });

    // Initial check
    if (!('usb' in navigator)) {
        if (connectBtn) {
            connectBtn.disabled = true;
        }
        setStatus('WebUSB not available in this browser.');
    }

    if ('usb' in navigator) {
        navigator.usb.addEventListener('connect', () => {
            autoConnect();
        });
        navigator.usb.addEventListener('disconnect', () => {
            handleDisconnect('Device disconnected.');
        });
    }
}
