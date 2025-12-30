
import init, { FlightDeck } from "/pkg/flight_deck.js";
import AsyncQueue from "/async_queue.js";

const statusEl = document.getElementById('status');
const connectBtn = document.getElementById('connect');
const colorBtns = ['red','green','blue','rainbow'].map(id => document.getElementById(id));
let writer = null;
const SEND_QUEUE_KEY = "__toSendQueue__";
let deck = null;

export async function initDeck() {
    console.log("initDeck");
    await init();
    globalThis[SEND_QUEUE_KEY] ??= new AsyncQueue();
    console.log("queue inited", globalThis[SEND_QUEUE_KEY]);
    deck = new FlightDeck();
    return deck;
}

export function send(message) {
    console.log("enqueue message", globalThis[SEND_QUEUE_KEY]);
    globalThis[SEND_QUEUE_KEY].enqueue(message);
}


export async function consumeQueue() {
    while (true) {
        console.log("consume loop");
        let message = await globalThis[SEND_QUEUE_KEY].dequeue();
        console.log("got message", message);
        await sendMessage(message);
    }
} 

function setStatus(msg) {
    statusEl.textContent = msg;
}

function enableControls() { 
    connectBtn.disabled = true; colorBtns.forEach(b => b.disabled = false);
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
    
    // Find and claim the first available interface
    const interfaces = device.configuration.interfaces;
    if (interfaces.length === 0) {
        throw new Error('No interfaces available');
    }

    await device.claimInterface(1);
        /*
        // Try to claim interface, handling if it's already claimed
        try {
            await device.claimInterface(interfaces[0].interfaceNumber);
        } catch (claimError) {
            console.warn('Interface claim failed, trying interface 0:', claimError);
            // Fallback to interface 1 if the first interface fails
            await device.claimInterface(1);
        }*/
        writer = { device, type: 'usb' };
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
            await writer.device.transferOut(2, message);
        }
        setStatus(`Sent ${message.length} bytes`);
    } catch (err) {
        console.error('Write error:', err);
        setStatus('Send failed: ' + (err.message || err));
    }
}


connectBtn.addEventListener('click', connect);

const colorCalls = [
    { r: 255, g: 0, b: 0 },
    { r: 0, g: 255, b: 0 },
    { r: 0, g: 0, b: 255 },
    { r: 255, g: 255, b: 0 },
];

colorCalls.forEach((color, i) => colorBtns[i].addEventListener('click', () => {
    if (!deck) {
        setStatus('Deck not initialized yet.');
        return;
    }
    console.log("calling function:", 0, 0, [color.r, color.g, color.b]);
    deck.call(0, 0, [color.r, color.g, color.b]);
}));

// Initial check
if (!('serial' in navigator) && !('usb' in navigator)) {
    connectBtn.disabled = true;
    setStatus('Neither Web Serial nor WebUSB available.');
}
