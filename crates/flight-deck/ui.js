import { consumeQueue, initDeck, autoConnect, callMachineFunction } from "/deck.js";

export class MachineControlDescriptor {
  constructor({
    id,
    label,
    functionId,
    type = "range",
    min = 0,
    max = 100,
    step = 1,
    defaultValue = 0,
    units = "",
  }) {
    this.id = id;
    this.label = label;
    this.functionId = functionId;
    this.type = type;
    this.min = min;
    this.max = max;
    this.step = step;
    this.defaultValue = defaultValue;
    this.units = units;
  }
}



export class MachineDescriptor {
  constructor({ id, name, assembly, controls = [] }) {
    this.id = id;
    this.name = name;
    this.assembly = assembly;
    this.controls = controls;
  }
}

function hexToRgb(hex) {
  if (!hex) {
    return null;
  }
  const normalized = hex.startsWith("#") ? hex.slice(1) : hex;
  if (normalized.length !== 6) {
    return null;
  }
  const r = Number.parseInt(normalized.slice(0, 2), 16);
  const g = Number.parseInt(normalized.slice(2, 4), 16);
  const b = Number.parseInt(normalized.slice(4, 6), 16);
  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) {
    return null;
  }
  return {
    r: Math.max(0, Math.min(255, r)),
    g: Math.max(0, Math.min(255, g)),
    b: Math.max(0, Math.min(255, b)),
  };
}

function findTrackHost(element) {
  let node = element;
  while (node) {
    const direct = node.closest?.("fd-track");
    if (direct) {
      return direct;
    }
    const root = node.getRootNode?.();
    node = root?.host ?? null;
  }
  return null;
}

function getTrackIndexForElement(element) {
  const track = findTrackHost(element);
  if (!track) {
    return null;
  }
  const trackList = document.getElementById("track-list");
  if (!trackList) {
    return null;
  }
  const tracks = Array.from(trackList.querySelectorAll("fd-track")).filter(
    (item) => {
      const machineId = item.dataset.machineId || "";
      const machineAssembly = item.dataset.machineAssembly || "";
      return Boolean(machineId || machineAssembly);
    }
  );
  const index = tracks.indexOf(track);
  return index >= 0 ? index : null;
}

function sendControlCall(element, control, args) {
  const machineIndex = getTrackIndexForElement(element);
  if (machineIndex == null) {
    return;
  }
  const functionId = Number(control.functionId);
  if (!Number.isFinite(functionId)) {
    return;
  }
  const payload = Array.isArray(args) ? args : [args];
  const sanitized = payload.map((value) => {
    const numberValue = Number(value);
    if (!Number.isFinite(numberValue)) {
      return 0;
    }
    return Math.max(0, Math.round(numberValue));
  });
  callMachineFunction(machineIndex, functionId, sanitized);
}

export const CRAWLER_MACHINE = `
.machine main globals 4 functions 5
    .global red 0
    .global green 1
    .global blue 2
    .global brightness 3

    .func init index 0
      PUSH 0
      STORE red
      PUSH 16
      STORE green
      PUSH 32
      STORE blue
      PUSH 100
      STORE brightness
      EXIT
    .end

    .func set_rgb index 2
      STORE 0
      STORE 1
      STORE 2
      EXIT
    .end

    .func set_brightness index 3
      STORE brightness
      EXIT
    .end

    .func get_rgb_worker index 4
      .frame led_index 0
      .frame ticks 1
      PUSH 1000 
      MOD ; count up 1 second
      DUP         
      SLOAD led_index ; stack is : led_index, ticks, ticks led index
      PUSH 40 ; ticks per LED (1000 / 25 LEDs)
      MUL          ; Compute adjusted led index
      DUP          
      SLOAD ticks ; Max distance [led_index, ticks, ticks, adjusted led, adjusted led, ticks]
      BRLTE before
      SWAP ; [led_index, ticks, adjusted led, ticks ]
      before:
      SUB [ led_index, ticks, distance ]
      DUP
      PUSH 128 ; Max distance [ led_index, ticks, distance, distance, 128 ]
      BRLTE close
      PUSH 0
      PUSH 0
      PUSH 0
      RET 3
      close: ; Compute scale factor 128 / tick_distance
      PUSH 128 ; invert
      SWAP
      SUB
      PUSH 128 ; Compute scale factor [ led_index, ticks, distance, 128]
      SWAP
      DIV
      DUP    ; Scale red
      LOAD red
      SWAP
      DIV
      LOAD brightness
      MUL
      PUSH 100
      DIV
      SWAP    ; Scale green
      DUP
      LOAD green
      SWAP
      DIV
      LOAD brightness
      MUL
      PUSH 100
      DIV
      SWAP    ; Scale blue
      LOAD blue
      SWAP
      DIV
      LOAD brightness
      MUL
      PUSH 100
      DIV
      RET 3
    .end

    .func get_rgb index 1
      PUSH 2
      CALL get_rgb_worker
      EXIT
    .end
.end
`;

export const SIMPLE_CRAWLER_MACHINE = `
.machine main globals 5 functions 6
    .global red 0
    .global green 1
    .global blue 2
    .global speed 3
    .global brightness 4

    .func init index 0
      PUSH 0
      STORE red
      PUSH 16
      STORE green
      PUSH 32
      STORE blue
      PUSH 100
      STORE speed
      PUSH 100
      STORE brightness
      EXIT
    .end

    .func set_rgb index 2
      STORE red
      STORE blue
      STORE green
      EXIT
    .end

    .func set_brightness index 3
      STORE brightness
      EXIT
    .end

    .func set_speed index 4
      STORE speed
      EXIT
    .end

    .func get_rgb_worker index 5
      .frame led_index 0
      .frame ticks 1
      LOAD speed
      MOD ; count up 1 second
      LOAD speed
      push 25
      DIV
      DIV
      BREQ match
      PUSH 0
      PUSH 0
      PUSH 0
      RET 3
      match: 
      LOAD red
      LOAD brightness
      MUL
      PUSH 100
      DIV
      LOAD green
      LOAD brightness
      MUL
      PUSH 100
      DIV
      LOAD blue
      LOAD brightness
      MUL
      PUSH 100
      DIV
      RET 3
    .end

    .func get_rgb index 1
      PUSH 2
      CALL get_rgb_worker
      EXIT
    .end



.end
`;



export const PULSE_MACHINE = `
.machine main globals 4 functions 4
    .global red 0
    .global green 1
    .global blue 2
    .global brightness 3

    .func init index 0
      PUSH 0
      STORE red
      PUSH 16
      STORE green
      PUSH 32
      STORE blue
      PUSH 100
      STORE brightness
      EXIT
    .end

    .func set_rgb index 2
      STORE 0
      STORE 1
      STORE 2
      EXIT
    .end

    .func set_brightness index 3
      STORE brightness
      EXIT
    .end

    .func get_rgb index 1 ; Stack [index, tick]
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
      LOAD brightness
      MUL
      PUSH 100
      DIV
      LOAD 1
      LOAD brightness
      MUL
      PUSH 100
      DIV
      LOAD 2
      LOAD brightness
      MUL
      PUSH 100
      DIV
      EXIT
    .end

.end
`;



export const DEFAULT_MACHINE_RACK = [
  new MachineDescriptor({
    id: "SimpleCrawlerMachine",
    name: "Simple Crawler",
    assembly: SIMPLE_CRAWLER_MACHINE,
    controls: [
      new MachineControlDescriptor({
        id: "speed",
        label: "Speed",
        functionId: 4,
        type: "range",
        min: 10,
        max: 1000,
        step: 10,
        defaultValue: 1000,
      }),
    ],
  }),
  new MachineDescriptor({
    id: "FixedColorMachine",
    name: "Fixed Color Machine",
    assembly: PULSE_MACHINE,
    controls: [
      new MachineControlDescriptor({
        id: "rainbow",
        label: "Pick Color",
        functionId: 2,
        type: "color_picker",
        defaultValue: "#468bc0ff",
      }),
    ],
  }),
  new MachineDescriptor({
    id: "CrawlerMachine",
    name: "Crawler",
    assembly: CRAWLER_MACHINE,
    controls: [],
  }),
];

const machineTpl = document.createElement("template");
machineTpl.innerHTML = `
  <link rel="stylesheet" href="index.css">
  <h3></h3>
  <p></p>
  <button class="machine-add" type="button"></button>
`;

const trackTpl = document.createElement("template");
trackTpl.innerHTML = `
  <link rel="stylesheet" href="index.css">
  <div class="track-machine">
    <div class="track-header">
      <strong class="track-name"></strong>
      <div class="track-actions">
        <div class="track-controls"></div>
        <button class="track-edit" type="button">Edit</button>
        <button class="track-delete" type="button">Remove</button>
      </div>
    </div>
    <span class="chip track-meta"></span>
    <div class="track-editor">
      <label>Assembly</label>
      <textarea spellcheck="false" placeholder=".machine main globals 0 functions 0"></textarea>
    </div>
  </div>
`;

const colorPickerTpl = document.createElement("template");
colorPickerTpl.innerHTML = `
  <style>
    :host {
      display: inline-flex;
      flex-direction: column;
      gap: 6px;
      font-family: "Trebuchet MS", "Gill Sans", "Verdana", sans-serif;
      color: #3b1f4c;
    }
    label {
      font-size: 11px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .row {
      display: inline-flex;
      align-items: center;
      gap: 10px;
    }
    input[type="color"] {
      width: 44px;
      height: 32px;
      border: 2px solid rgba(122, 90, 158, 0.6);
      border-radius: 10px;
      background: #ffffff;
      padding: 2px;
      cursor: pointer;
    }
    .value {
      font-size: 12px;
      font-family: "Courier New", monospace;
      color: #6a3a74;
    }
  </style>
  <label part="label"></label>
  <div class="row">
    <input part="input" type="color" />
    <span class="value" part="value"></span>
  </div>
`;

const rangeControlTpl = document.createElement("template");
rangeControlTpl.innerHTML = `
  <style>
    :host {
      display: inline-flex;
      font-family: var(--font-display, "Trebuchet MS", "Gill Sans", "Verdana", sans-serif);
      color: #3b1f4c;
    }
    .range-pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 10px;
      border-radius: 999px;
      border: 2px solid rgba(126, 96, 158, 0.42);
      background: rgba(214, 238, 228, 0.9);
      color: #2b2137;
      font-size: 12px;
      letter-spacing: 0.02em;
      cursor: ew-resize;
    }
    .range-pill:focus-visible {
      outline: 2px solid rgba(92, 64, 124, 0.7);
      outline-offset: 2px;
    }
    .value {
      font-family: "Courier New", monospace;
      font-size: 11px;
      color: #4a2b63;
    }
    .scrub-overlay {
      position: fixed;
      inset: 0;
      background: rgba(24, 16, 34, 0.28);
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 999;
    }
    .scrub-overlay[hidden] {
      display: none;
    }
    .scrub-card {
      min-width: min(320px, 80vw);
      padding: 16px;
      border-radius: 16px;
      border: 2px solid rgba(106, 78, 136, 0.6);
      background: rgba(244, 230, 252, 0.96);
      box-shadow: 0 20px 40px rgba(42, 26, 58, 0.22);
      display: grid;
      gap: 8px;
      text-align: center;
    }
    .scrub-title {
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.12em;
      color: rgba(58, 30, 75, 0.7);
    }
    .scrub-track {
      position: relative;
      height: 16px;
      border-radius: 999px;
      background: rgba(120, 90, 150, 0.2);
      overflow: hidden;
    }
    .scrub-fill {
      height: 100%;
      background: linear-gradient(90deg, rgba(122, 90, 158, 0.7), rgba(160, 120, 196, 0.9));
      width: 0%;
    }
    .scrub-value {
      font-family: "Courier New", monospace;
      font-size: 16px;
      color: #2b2137;
    }
    .scrub-hint {
      font-size: 11px;
      color: rgba(58, 30, 75, 0.6);
    }
  </style>
  <button class="range-pill" type="button" role="slider">
    <span class="label"></span>
    <span class="value"></span>
  </button>
  <div class="scrub-overlay" hidden>
    <div class="scrub-card">
      <div class="scrub-title"></div>
      <div class="scrub-track">
        <div class="scrub-fill"></div>
      </div>
      <div class="scrub-value"></div>
      <div class="scrub-hint">Drag left to lower, right to raise</div>
    </div>
  </div>
`;

const statusTpl = document.createElement("template");
statusTpl.innerHTML = `
  <link rel="stylesheet" href="index.css">
  <div class="status-pill">
    <span class="status-dot"></span>
    <span class="status-text"><slot></slot></span>
  </div>
`;
class MachineCard extends HTMLElement {
  static get observedAttributes() {
    return ["title", "description", "action-label"];
  }

  #titleEl;
  #descEl;
  #btnEl;

  constructor() {
    super();

    // Shadow DOM is optional, but usually recommended for components.
    const root = this.attachShadow({ mode: "open" });
    root.append(machineTpl.content.cloneNode(true));

    // Cache references once
    this.#titleEl = root.querySelector("h3");
    this.#descEl = root.querySelector("p");
    this.#btnEl = root.querySelector("button");

    // Attach event listeners once (they won't be wiped by renders)
    this.#btnEl.addEventListener("click", () => {
      this.dispatchEvent(new CustomEvent("add", { bubbles: true, composed: true }));
    });
  }

  get getTitle() {
    return this.getAttribute("title") ??
      this.#titleEl?.textContent?.trim() ??
      "Machine";
  }

  get getDesc() {
    return this.getAttribute("description") ??
      card.#descEl?.textContent?.trim() ??
      "";
  }

  connectedCallback() {
    this.classList.add("machine-card");
    if (!this.hasAttribute("draggable")) {
      this.setAttribute("draggable", "true");
    }
    this.render();
  }

  attributeChangedCallback() {
    this.render();
  }

  render() {
    const title = this.getAttribute("title") ?? "Machine";
    const description = this.getAttribute("description") ?? "";
    const actionLabel = this.getAttribute("action-label") ?? "Add to Tracks";

    this.#titleEl.textContent = title;
    this.#descEl.textContent = description;
    this.#btnEl.textContent = actionLabel;
  }
}

class MachineRack extends HTMLElement {
  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.innerHTML = `<slot></slot>`;
  }

  connectedCallback() {
    this.classList.add("machine-rack");
  }
}

if (!customElements.get("fd-machine-card")) {
  customElements.define("fd-machine-card", MachineCard);
}

if (!customElements.get("fd-machine-rack")) {
  customElements.define("fd-machine-rack", MachineRack);
}

class TrackMachine extends HTMLElement {
  static get observedAttributes() {
    return ["name", "meta"];
  }

  #nameEl;
  #metaEl;
  #controlsEl;
  #removeBtn;
  #editBtn;
  #editorWrap;
  #editorField;
  #isExpanded = false;
  #controls = [];

  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.append(trackTpl.content.cloneNode(true));
    this.#nameEl = root.querySelector(".track-name");
    this.#metaEl = root.querySelector(".track-meta");
    this.#controlsEl = root.querySelector(".track-controls");
    this.#removeBtn = root.querySelector(".track-delete");
    this.#editBtn = root.querySelector(".track-edit");
    this.#editorWrap = root.querySelector(".track-editor");
    this.#editorField = root.querySelector("textarea");
    this.#removeBtn.addEventListener("click", () => {
      this.dispatchEvent(
        new CustomEvent("track-remove", { bubbles: true, composed: true })
      );
    });
    this.#editBtn.addEventListener("click", () => {
      this.toggleEditor();
    });
    this.#editorField.addEventListener("input", () => {
      const track = this.closest("fd-track");
      if (!track) {
        return;
      }
      track.dataset.machineAssembly = this.#editorField.value;
      track.dataset.machineSource = "manual";
    });
  }

  connectedCallback() {
    this.render();
    this.syncEditor();
  }

  attributeChangedCallback() {
    this.render();
  }

  get controls() {
    return this.#controls;
  }

  set controls(next) {
    this.#controls = Array.isArray(next) ? next : [];
    this.renderControls();
  }

  render() {
    const name = this.getAttribute("name") ?? "Machine";
    const meta = this.getAttribute("meta") ?? "Machine";
    this.#nameEl.textContent = name;
    this.#metaEl.textContent = meta;
    this.renderControls();
  }

  renderControls() {
    if (!this.#controlsEl) {
      return;
    }
    this.#controlsEl.innerHTML = "";
    this.#controls.forEach((control) => {
      if (control.type === "range") {
        const rangeControl = document.createElement("fd-range-control");
        rangeControl.setAttribute("label", control.label ?? "");
        rangeControl.setAttribute("control-id", control.id ?? "");
        rangeControl.setAttribute("min", String(control.min ?? 0));
        rangeControl.setAttribute("max", String(control.max ?? 100));
        rangeControl.setAttribute("step", String(control.step ?? 1));
        rangeControl.setAttribute(
          "value",
          String(control.defaultValue ?? control.min ?? 0)
        );
        if (control.units) {
          rangeControl.setAttribute("units", String(control.units));
        }
        rangeControl.addEventListener("input", (event) => {
          const value =
            event?.detail?.value ??
            Number(rangeControl.getAttribute("value") ?? 0);
          sendControlCall(rangeControl, control, value);
        });
        rangeControl.addEventListener("change", (event) => {
          const value =
            event?.detail?.value ??
            Number(rangeControl.getAttribute("value") ?? 0);
          sendControlCall(rangeControl, control, value);
        });
        this.#controlsEl.appendChild(rangeControl);
        return;
      }
      if (control.type === "color_picker") {
        const colorPicker = document.createElement("fd-color-picker");
        colorPicker.setAttribute("label", control.label ?? "");
        colorPicker.setAttribute("control-id", control.id ?? "");
        if (typeof control.defaultValue === "string") {
          colorPicker.setAttribute("value", control.defaultValue);
        }
        const handleColorEvent = (event) => {
          const value = event?.detail?.value ?? colorPicker.value;
          const rgb = hexToRgb(value);
          if (!rgb) {
            return;
          }
          sendControlCall(colorPicker, control, [rgb.r, rgb.g, rgb.b]);
        };
        colorPicker.addEventListener("input", handleColorEvent);
        colorPicker.addEventListener("change", handleColorEvent);
        this.#controlsEl.appendChild(colorPicker);
        return;
      }
      const chip = document.createElement("span");
      chip.className = "chip";
      const showValue =
        control.type !== "color_picker" &&
        typeof control.defaultValue === "number";
      const valueText = showValue
        ? ` ${control.defaultValue}${control.units ?? ""}`
        : "";
      chip.textContent = `${control.label}${valueText}`;
      this.#controlsEl.appendChild(chip);
    });
  }

  toggleEditor() {
    this.#isExpanded = !this.#isExpanded;
    this.#editorWrap.classList.toggle("is-open", this.#isExpanded);
    this.#editBtn.textContent = this.#isExpanded ? "Close" : "Edit";
    if (this.#isExpanded) {
      this.#editorField.focus();
      this.#editorField.selectionStart = this.#editorField.value.length;
      this.#editorField.selectionEnd = this.#editorField.value.length;
    }
  }

  syncEditor() {
    const track = this.closest("fd-track");
    if (!track) {
      return;
    }
    const assembly = track.dataset.machineAssembly || "";
    if (this.#editorField.value !== assembly) {
      this.#editorField.value = assembly;
    }
  }
}

if (!customElements.get("fd-track-machine")) {
  customElements.define("fd-track-machine", TrackMachine);
}

export { MachineCard, MachineRack, TrackMachine };

class ColorPicker extends HTMLElement {
  static get observedAttributes() {
    return ["value", "label"];
  }

  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.append(colorPickerTpl.content.cloneNode(true));
    this.inputEl = root.querySelector("input");
    this.labelEl = root.querySelector("label");
    this.valueEl = root.querySelector(".value");
  }

  connectedCallback() {
    if (!this.hasAttribute("value")) {
      this.value = "#c5a1ef";
    }
    if (!this.hasAttribute("label")) {
      this.label = "Color";
    }
    this.inputEl.addEventListener("input", this.handleInput);
    this.inputEl.addEventListener("change", this.handleChange);
    this.syncFromAttributes();
  }

  disconnectedCallback() {
    this.inputEl.removeEventListener("input", this.handleInput);
    this.inputEl.removeEventListener("change", this.handleChange);
  }

  attributeChangedCallback() {
    this.syncFromAttributes();
  }

  get value() {
    return this.getAttribute("value");
  }

  set value(next) {
    this.setAttribute("value", next);
  }

  get label() {
    return this.getAttribute("label");
  }

  set label(next) {
    this.setAttribute("label", next);
  }

  handleInput = (event) => {
    const next = event.target.value;
    this.value = next;
    this.dispatchEvent(new CustomEvent("input", { detail: { value: next } }));
  };

  handleChange = (event) => {
    const next = event.target.value;
    this.value = next;
    this.dispatchEvent(new CustomEvent("change", { detail: { value: next } }));
  };

  syncFromAttributes() {
    const value = this.getAttribute("value") || "#c5a1ef";
    const label = this.getAttribute("label") || "Color";
    if (this.inputEl.value !== value) {
      this.inputEl.value = value;
    }
    this.labelEl.textContent = label;
    this.valueEl.textContent = value.toUpperCase();
  }
}

if (!customElements.get("fd-color-picker")) {
  customElements.define("fd-color-picker", ColorPicker);
}

class RangeControl extends HTMLElement {
  static get observedAttributes() {
    return ["value", "label", "min", "max", "step", "units"];
  }

  #buttonEl;
  #labelEl;
  #valueEl;
  #overlayEl;
  #overlayCardEl;
  #overlayTitleEl;
  #overlayValueEl;
  #fillEl;
  #startX = 0;
  #startValue = 0;
  #activePointerId = null;
  #pixelsPerStep = 6;

  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.append(rangeControlTpl.content.cloneNode(true));
    this.#buttonEl = root.querySelector(".range-pill");
    this.#labelEl = root.querySelector(".label");
    this.#valueEl = root.querySelector(".value");
    this.#overlayEl = root.querySelector(".scrub-overlay");
    this.#overlayCardEl = root.querySelector(".scrub-card");
    this.#overlayTitleEl = root.querySelector(".scrub-title");
    this.#overlayValueEl = root.querySelector(".scrub-value");
    this.#fillEl = root.querySelector(".scrub-fill");
  }

  connectedCallback() {
    if (!this.hasAttribute("value")) {
      this.value = String(this.min);
    }
    if (this.#overlayEl) {
      this.#overlayEl.hidden = true;
    }
    this.#buttonEl.addEventListener("pointerdown", this.handlePointerDown);
    this.#buttonEl.addEventListener("keydown", this.handleKeyDown);
    this.syncFromAttributes();
  }

  disconnectedCallback() {
    this.#buttonEl.removeEventListener("pointerdown", this.handlePointerDown);
    this.#buttonEl.removeEventListener("keydown", this.handleKeyDown);
    this.teardownPointerTracking();
  }

  attributeChangedCallback() {
    this.syncFromAttributes();
  }

  get value() {
    return this.getAttribute("value");
  }

  set value(next) {
    this.setAttribute("value", next);
  }

  get label() {
    return this.getAttribute("label");
  }

  set label(next) {
    this.setAttribute("label", next);
  }

  get units() {
    return this.getAttribute("units");
  }

  set units(next) {
    if (next == null) {
      this.removeAttribute("units");
      return;
    }
    this.setAttribute("units", next);
  }

  get min() {
    return this.parseNumberAttr("min", 0);
  }

  get max() {
    return this.parseNumberAttr("max", 100);
  }

  get step() {
    const raw = this.parseNumberAttr("step", 1);
    return raw > 0 ? raw : 1;
  }

  parseNumberAttr(name, fallback) {
    const raw = this.getAttribute(name);
    if (raw === null || raw === "") {
      return fallback;
    }
    const value = Number(raw);
    return Number.isFinite(value) ? value : fallback;
  }

  formatValue(value) {
    const units = this.units ?? "";
    return `${value}${units}`;
  }

  clampValue(value) {
    const min = this.min;
    const max = this.max;
    return Math.max(min, Math.min(max, value));
  }

  normalizeValue(value) {
    const step = this.step;
    const min = this.min;
    const snapped = Math.round((value - min) / step) * step + min;
    return this.clampValue(snapped);
  }

  syncFromAttributes() {
    const label = this.label || "Control";
    const value = this.normalizeValue(this.parseNumberAttr("value", this.min));
    const min = this.min;
    const max = this.max;
    if (this.#labelEl.textContent !== label) {
      this.#labelEl.textContent = label;
    }
    const formatted = this.formatValue(value);
    this.#valueEl.textContent = formatted;
    this.#overlayTitleEl.textContent = label;
    this.#overlayValueEl.textContent = formatted;
    const percent = max > min ? ((value - min) / (max - min)) * 100 : 0;
    this.#fillEl.style.width = `${percent}%`;
    this.#buttonEl.setAttribute("aria-valuemin", String(min));
    this.#buttonEl.setAttribute("aria-valuemax", String(max));
    this.#buttonEl.setAttribute("aria-valuenow", String(value));
    this.#buttonEl.setAttribute("aria-valuetext", formatted);
  }

  updateValue(next, { emit = true } = {}) {
    const normalized = this.normalizeValue(next);
    if (String(normalized) !== this.value) {
      this.value = String(normalized);
      if (emit) {
        this.dispatchEvent(
          new CustomEvent("input", {
            detail: { value: normalized },
            bubbles: true,
            composed: true,
          })
        );
      }
    }
    this.syncFromAttributes();
  }

  handlePointerDown = (event) => {
    if (event.button !== undefined && event.button !== 0) {
      return;
    }
    event.preventDefault();
    this.#activePointerId = event.pointerId;
    this.#startX = event.clientX;
    this.#startValue = this.normalizeValue(
      this.parseNumberAttr("value", this.min)
    );
    this.#pixelsPerStep = this.computePixelsPerStep();
    this.#overlayEl.hidden = false;
    this.updateOverlayWidth();
    this.#buttonEl.setPointerCapture?.(event.pointerId);
    window.addEventListener("pointermove", this.handlePointerMove);
    window.addEventListener("pointerup", this.handlePointerUp);
    window.addEventListener("pointercancel", this.handlePointerUp);
  };

  handlePointerMove = (event) => {
    if (this.#activePointerId !== event.pointerId) {
      return;
    }
    const deltaX = event.clientX - this.#startX;
    const stepsDelta = Math.round(deltaX / this.#pixelsPerStep);
    const next = this.#startValue + stepsDelta * this.step;
    this.updateValue(next);
  };

  handlePointerUp = (event) => {
    if (this.#activePointerId !== event.pointerId) {
      return;
    }
    this.dispatchEvent(
      new CustomEvent("change", {
        detail: { value: this.parseNumberAttr("value", this.min) },
        bubbles: true,
        composed: true,
      })
    );
    this.teardownPointerTracking();
  };

  handleKeyDown = (event) => {
    const key = event.key;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(key)) {
      return;
    }
    event.preventDefault();
    const current = this.parseNumberAttr("value", this.min);
    let next = current;
    if (key === "ArrowLeft") {
      next = current - this.step;
    } else if (key === "ArrowRight") {
      next = current + this.step;
    } else if (key === "Home") {
      next = this.min;
    } else if (key === "End") {
      next = this.max;
    }
    this.updateValue(next);
    this.dispatchEvent(
      new CustomEvent("change", {
        detail: { value: this.parseNumberAttr("value", this.min) },
        bubbles: true,
        composed: true,
      })
    );
  };

  teardownPointerTracking() {
    if (this.#overlayEl) {
      this.#overlayEl.hidden = true;
    }
    if (this.#activePointerId !== null) {
      try {
        this.#buttonEl.releasePointerCapture?.(this.#activePointerId);
      } catch (err) {
        // Ignore if capture already released.
      }
    }
    this.#activePointerId = null;
    window.removeEventListener("pointermove", this.handlePointerMove);
    window.removeEventListener("pointerup", this.handlePointerUp);
    window.removeEventListener("pointercancel", this.handlePointerUp);
  }

  computePixelsPerStep() {
    const range = Math.max(0, this.max - this.min);
    const steps = Math.max(1, range / this.step);
    return Math.max(3, 240 / steps);
  }

  updateOverlayWidth() {
    if (!this.#overlayCardEl) {
      return;
    }
    const track = this.findTrackHost();
    if (!track) {
      this.#overlayCardEl.style.width = "";
      return;
    }
    const rect = track.getBoundingClientRect();
    if (!rect?.width) {
      this.#overlayCardEl.style.width = "";
      return;
    }
    this.#overlayCardEl.style.width = `${Math.round(rect.width * 0.8)}px`;
  }

  findTrackHost() {
    let node = this;
    while (node) {
      const direct = node.closest?.("fd-track");
      if (direct) {
        return direct;
      }
      const root = node.getRootNode?.();
      node = root?.host ?? null;
    }
    return null;
  }
}

if (!customElements.get("fd-range-control")) {
  customElements.define("fd-range-control", RangeControl);
}

class StatusPill extends HTMLElement {
  static get observedAttributes() {
    return ["state"];
  }

  #dotEl;
  #pillEl;

  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.append(statusTpl.content.cloneNode(true));
    this.#dotEl = root.querySelector(".status-dot");
    this.#pillEl = root.querySelector(".status-pill");
  }

  connectedCallback() {
    this.syncState();
  }

  attributeChangedCallback() {
    this.syncState();
  }

  syncState() {
    const state = this.getAttribute("state") ?? "disconnected";
    const isConnected = state === "connected";
    this.#pillEl.classList.toggle("is-connected", isConnected);
    this.#pillEl.classList.toggle("is-disconnected", !isConnected);
  }
}

if (!customElements.get("fd-status")) {
  customElements.define("fd-status", StatusPill);
}

export { ColorPicker, StatusPill };

export async function runUi() {
  const trackList = document.getElementById("track-list");
  const machineCards = document.querySelectorAll("fd-machine-card");
  const addTrackBtn = document.getElementById("add-track");
  const closeRackBtn = document.getElementById("close-rack");
  const rackScrim = document.getElementById("rack-scrim");
  const shell = document.querySelector("fd-app");
  const rackPanel = document.querySelector(".machine-rack");
  let dragPayload = null;
  const machineRegistry = new Map(
    DEFAULT_MACHINE_RACK.map((machine) => [machine.id, machine])
  );
  const defaultMachine = DEFAULT_MACHINE_RACK[0];

  const extractMachineInfo = (card) => {
    const machineId = card.dataset.machineId || "";
    const machineSource = card.dataset.machineSource || "";
    const machine = machineRegistry.get(machineId);
    const title = card.getAttribute("title") ?? "Machine";
    let name = title;
    let meta = "Rack Copy";
    if (title.includes("·")) {
      const parts = title.split("·");
      name = parts.slice(1).join("·").trim() || title;
      meta = parts[0].trim();
    }
    const desc = card.getAttribute("description") ?? "";
    return {
      name,
      meta,
      desc,
      machineId,
      assembly: machine?.assembly ?? "",
      controls: machine?.controls ?? [],
      source: machineSource,
    };
  };

  const createTrack = ({ name, meta, machineId, assembly, controls, source }) => {
    const track = document.createElement("fd-track");
    const machine = document.createElement("fd-track-machine");
    machine.setAttribute("name", name);
    machine.setAttribute("meta", `${meta} · from rack`);
    machine.controls = controls;
    if (machineId) {
      track.dataset.machineId = machineId;
    }
    if (source) {
      track.dataset.machineSource = source;
    }
    if (assembly) {
      track.dataset.machineAssembly = assembly;
    }
    track.appendChild(machine);
    return track;
  };

  const addTrackFromCard = (card) => {
    if (!trackList) {
      return;
    }
    const info = extractMachineInfo(card);
    const newTrack = createTrack(info);
    trackList.appendChild(newTrack);
  };

  const hydrateDefaultTrack = () => {
    if (!trackList || !defaultMachine) {
      return;
    }
    const firstTrack = trackList.querySelector("fd-track");
    if (!firstTrack || firstTrack.dataset.machineId) {
      return;
    }
    const trackMachine = firstTrack.querySelector("fd-track-machine");
    if (trackMachine) {
      trackMachine.setAttribute("name", defaultMachine.name);
      trackMachine.setAttribute("meta", "Rack Default · preloaded");
      trackMachine.controls = defaultMachine.controls ?? [];
    }
    firstTrack.dataset.machineId = defaultMachine.id;
    firstTrack.dataset.machineAssembly = defaultMachine.assembly || "";
  };

  if (trackList) {
    trackList.addEventListener("track-remove", (event) => {
      const track = event.target.closest("fd-track");
      if (track) {
        track.remove();
      }
    });
    trackList.addEventListener("dragover", (event) => {
      event.preventDefault();
      trackList.classList.add("drag-over");
    });
    trackList.addEventListener("dragleave", () => {
      trackList.classList.remove("drag-over");
    });
    trackList.addEventListener("drop", (event) => {
      event.preventDefault();
      trackList.classList.remove("drag-over");
      if (!dragPayload) {
        return;
      }
      const newTrack = createTrack(dragPayload);
      trackList.appendChild(newTrack);
      dragPayload = null;
    });
  }

  hydrateDefaultTrack();

  machineCards.forEach((card) => {
    card.addEventListener("dragstart", (event) => {
      dragPayload = extractMachineInfo(card);
      event.dataTransfer?.setData("text/plain", dragPayload.name);
      event.dataTransfer?.setData("text/description", dragPayload.desc);
      event.dataTransfer?.setDragImage(card, 16, 16);
      card.classList.add("dragging");
    });
    card.addEventListener("dragend", () => {
      card.classList.remove("dragging");
    });
  });

  rackPanel?.addEventListener("add", (event) => {
    const card = event.target.closest("fd-machine-card");
    if (!card) {
      return;
    }
    addTrackFromCard(card);
  });

  const openRack = () => {
    shell?.classList.add("rack-open");
    rackPanel?.setAttribute("aria-hidden", "false");
  };

  const closeRack = () => {
    shell?.classList.remove("rack-open");
    rackPanel?.setAttribute("aria-hidden", "true");
  };

  addTrackBtn?.addEventListener("click", openRack);
  closeRackBtn?.addEventListener("click", closeRack);
  rackScrim?.addEventListener("click", closeRack);

  await initDeck();
  await autoConnect();
  await consumeQueue();
}
