
import init, { FlightDeck, compile_program } from "/pkg/flight_deck.js";
import AsyncQueue from "/async_queue.js";

const statusEl = document.getElementById('status');
const connectBtn = document.getElementById('connect');
const statusPillEl = document.getElementById('status-pill');
const loadProgramBtn = document.getElementById('load-program');
const sliderEl = document.getElementById('color-slider');
const sliderValueEl = document.getElementById('slider-value');
const brightnessEl = document.getElementById('brightness-slider');
const brightnessValueEl = document.getElementById('brightness-value');
const editorEl = document.getElementById('program-editor');
const colorBtns = ['red','green','blue'].map(id => document.getElementById(id));
let writer = null;
const SEND_QUEUE_KEY = "__toSendQueue__";
const DECK_KEY = "__flightDeck__";
let usbReadActive = false;
let pendingRequestId = null;
let pendingStartTime = 0;
let pendingTimer = null;
let pendingColor = null;
let currentColor = { r: 0, g: 0, b: 0 };

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
}

const receiveHandler = new DeckReceiveHandler();

export async function initDeck() {
    await init();
    globalThis[SEND_QUEUE_KEY] ??= new AsyncQueue();
    globalThis[DECK_KEY] ??= new FlightDeck();
    if (editorEl && !editorEl.value.trim()) {
        editorEl.value = `
.machine main globals 3 functions 2

.func set_rgb index 0
    STORE 0
    STORE 1
    STORE 2
    EXIT
.end

.func get_rgb index 1
    DUP
    PUSH 120
    MOD
    SWAP
    PUSH 120
    DIV
    PUSH 2
    MOD
    PUSH 0
    BREQ even
    PUSH 120
    SWAP
    SUB
    even:
    PUSH 4
    DIV
    LOAD 0
    ADD
    LOAD 1
    LOAD 2
    EXIT
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

function setStatus(msg) {
    statusEl.textContent = msg;
}

function setConnectionState(isConnected) {
    if (!statusPillEl) {
        return;
    }
    statusPillEl.setAttribute('state', isConnected ? 'connected' : 'disconnected');
}

function enableControls() { 
    connectBtn.disabled = true;
    loadProgramBtn.disabled = false;
    sliderEl.disabled = false;
    brightnessEl.disabled = false;
    colorBtns.forEach(b => b.disabled = false);
}

function disableControls() {
    connectBtn.disabled = false;
    loadProgramBtn.disabled = true;
    sliderEl.disabled = true;
    brightnessEl.disabled = true;
    colorBtns.forEach(b => b.disabled = true);
}

function handleDisconnect(message) {
    writer = null;
    setConnectionState(false);
    disableControls();
    if (message) {
        setStatus(message);
    }
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
    if (pendingColor) {
        const next = pendingColor;
        pendingColor = null;
        scheduleSend(next);
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
            if (pendingColor) {
                pendingStartTime = performance.now();
                const next = pendingColor;
                pendingColor = null;
                scheduleSend(next);
            }
        }
    }, 200);
}

function sliderToRgb(value) {
    const clamped = Math.max(0, Math.min(1023, value));
    const t = clamped / 1023;
    let r = 0;
    let g = 0;
    let b = 0;
    if (t <= 0.5) {
        const p = t / 0.5;
        r = 0;
        g = Math.round(255 * p);
        b = Math.round(255 * (1 - p));
    } else {
        const p = (t - 0.5) / 0.5;
        r = Math.round(255 * p);
        g = Math.round(255 * (1 - p));
        b = 0;
    }
    return { r, g, b };
}

function brightnessScale() {
    const value = Number(brightnessEl?.value ?? 100);
    const clamped = Math.max(0, Math.min(100, value));
    return clamped / 100;
}

function applyBrightness(color) {
    const scale = brightnessScale();
    return {
        r: Math.round(color.r * scale),
        g: Math.round(color.g * scale),
        b: Math.round(color.b * scale),
    };
}

function sendSliderColor(color) {
    let deck = globalThis[DECK_KEY];
    if (!deck) {
        setStatus('Deck not initialized yet.');
        return;
    }
    const scaled = applyBrightness(color);
    pendingStartTime = performance.now();
    const requestId = deck.call(0, 0, [scaled.r, scaled.g, scaled.b]);
    if (requestId === undefined || requestId === null) {
        return;
    }
    pendingRequestId = requestId;
    setPendingTimeout();
}

function scheduleSend(color) {
    setTimeout(() => {
        sendSliderColor(color);
    }, 0);
}

export async function connect() {
    setStatus('Connecting...');
    // Try Web Serial first, fallback to WebUSB if not available
    if (false && 'serial' in navigator) {
        try {
            const port = await navigator.serial.requestPort({ filters: [] });
            await port.open({ baudRate: 9600 });
            writer = { port: port.writable.getWriter(), type: 'serial' };
            enableControls();
            setConnectionState(true);
            setStatus('Connected via Web Serial. Tap a color.');
            port.addEventListener?.('disconnect', () => {
                handleDisconnect('Device disconnected.');
            });
            return;
        } catch (err) {
            console.error('Web Serial Connect error:', err);
            setStatus('Web Serial failed, trying WebUSB...');
        }
    }
    
    // Fallback to WebUSB
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
        ];
        
        const device = await navigator.usb.requestDevice({ filters });
        await device.open();
        
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
        /*
        // Try to claim interface, handling if it's already claimed
        try {
            await device.claimInterface(interfaces[0].interfaceNumber);
        } catch (claimError) {
            console.warn('Interface claim failed, trying interface 0:', claimError);
            // Fallback to interface 1 if the first interface fails
            await device.claimInterface(1);
        }*/
        writer = { device, type: 'usb', inEndpoint, outEndpoint };
        startUsbReceiveLoop(device);
        enableControls();
        setConnectionState(true);
        setStatus('Connected via WebUSB. Tap a color.');
    } catch (err) {
        console.error('Connect error:', err);
        setConnectionState(false);
        disableControls();
        setStatus('Connection failed: ' + (err.message || err));
    }
}

async function sendMessage(message) {
    if (!writer) { setStatus('Device not connected.'); return; }
    try {
        if (writer.type === 'serial') {
            await writer.port.write(message);
        } else if (writer.type === 'usb') {
            await writer.device.transferOut(writer.outEndpoint, message);
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
                const copy = new Uint8Array(data);
                let deck = globalThis[DECK_KEY];
                if (!deck) {
                    setStatus('Deck not initialized yet.');
                    continue;
                }
                deck.receive(copy, receiveHandler);
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

connectBtn.addEventListener('click', connect);
loadProgramBtn.addEventListener('click', () => {
    let deck = globalThis[DECK_KEY];
    if (!deck) {
        setStatus('Deck not initialized yet.');
        return;
    }
    if (!editorEl) {
        setStatus('Program editor not available.');
        return;
    }
    const programBuffer = new Uint16Array(512);
    try {
        console.log("loading program", editorEl.value);
        const descriptor = compile_program(editorEl.value, programBuffer);
        deck.load_program(programBuffer, descriptor.length);
        setStatus(`Loaded program (${descriptor.length} words)`);
    } catch (err) {
        console.error('Load program error:', err);
        setStatus('Load program failed: ' + (err.message || err));
    }
});

sliderEl.addEventListener('input', () => {
    const value = Number(sliderEl.value);
    sliderValueEl.textContent = `${value}`;
    const color = sliderToRgb(value);
    currentColor = color;
    
    if (pendingRequestId !== null) {
        pendingColor = color;
    } else {
        sendSliderColor(color);
    }
});

brightnessEl.addEventListener('input', () => {
    const value = Number(brightnessEl.value);
    const clamped = Math.max(0, Math.min(100, value));
    brightnessValueEl.textContent = `${clamped}%`;
    if (pendingRequestId !== null) {
        pendingColor = currentColor;
    } else {
        sendSliderColor(currentColor);
    }
});

const colorCalls = [
    { r: 255, g: 0, b: 0 },
    { r: 0, g: 255, b: 0 },
    { r: 0, g: 0, b: 255 },
];

colorCalls.forEach((color, i) => colorBtns[i].addEventListener('click', () => {
    let deck = globalThis[DECK_KEY];
    if (!deck) {
        setStatus('Deck not initialized yet???');
        return;
    }
    currentColor = color;
    const scaled = applyBrightness(color);
    const request_id = deck.call(0, 0, [scaled.r, scaled.g, scaled.b]);
}));

// Initial check
if (!('serial' in navigator) && !('usb' in navigator)) {
    connectBtn.disabled = true;
    setStatus('Neither Web Serial nor WebUSB available.');
}

if ('usb' in navigator) {
    navigator.usb.addEventListener('disconnect', () => {
        handleDisconnect('Device disconnected.');
    });
}
