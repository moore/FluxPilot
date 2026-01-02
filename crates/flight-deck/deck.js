
import init, { FlightDeck, get_test_program } from "/pkg/flight_deck.js";
import AsyncQueue from "/async_queue.js";

const statusEl = document.getElementById('status');
const connectBtn = document.getElementById('connect');
const loadProgramBtn = document.getElementById('load-program');
const sliderEl = document.getElementById('color-slider');
const sliderValueEl = document.getElementById('slider-value');
const colorBtns = ['red','green','blue'].map(id => document.getElementById(id));
let writer = null;
const SEND_QUEUE_KEY = "__toSendQueue__";
const DECK_KEY = "__flightDeck__";
let usbReadActive = false;
let pendingRequestId = null;
let pendingTimer = null;
let pendingColor = null;

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

function enableControls() { 
    connectBtn.disabled = true;
    loadProgramBtn.disabled = false;
    sliderEl.disabled = false;
    colorBtns.forEach(b => b.disabled = false);
}

function resolvePending(requestId) {
    if (pendingRequestId === null || pendingRequestId !== requestId) {
        return false;
    }
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

function sendSliderColor(color) {
    let deck = globalThis[DECK_KEY];
    if (!deck) {
        setStatus('Deck not initialized yet.');
        return;
    }
    const requestId = deck.call(0, 0, [color.r, color.g, color.b]);
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
    setStatus('');
    // Try Web Serial first, fallback to WebUSB if not available
    if (false && 'serial' in navigator) {
        try {
            const port = await navigator.serial.requestPort({ filters: [] });
            await port.open({ baudRate: 9600 });
            writer = { port: port.writable.getWriter(), type: 'serial' };
            enableControls();
            setStatus('Connected via Web Serial. Tap a color.');
            return;
        } catch (err) {
            console.error('Web Serial Connect error:', err);
            setStatus('Web Serial failed, trying WebUSB...');
        }
    }
    
    // Fallback to WebUSB
    if (!('usb' in navigator)) {
        setStatus('Neither Web Serial nor WebUSB available.');
        return;
    }
    try {
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
        setStatus('Connected via WebUSB. Tap a color.');
    } catch (err) {
        console.error('Connect error:', err);
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
        setStatus('Send failed: ' + (err.message || err));
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
    const programBuffer = new Uint16Array(100);
    try {
        const descriptor = get_test_program(programBuffer);
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
    if (pendingRequestId !== null) {
        pendingColor = color;
        return;
    }
    sendSliderColor(color);
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
    const request_id = deck.call(0, 0, [color.r, color.g, color.b]);
}));

// Initial check
if (!('serial' in navigator) && !('usb' in navigator)) {
    connectBtn.disabled = true;
    setStatus('Neither Web Serial nor WebUSB available.');
}
