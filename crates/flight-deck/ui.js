import { consumeQueue, initDeck, autoConnect } from "/deck.js";

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


export const CRAWLER_MACHINE = `
.machine main globals 3 functions 3
    .global red 0
    .global green 1
    .global blue 2

    .func set_rgb index 0
      STORE 0
      STORE 1
      STORE 2
      EXIT
    .end

    .func get_rgb_worker index 2
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
      SWAP    ; Scale green
      DUP
      LOAD green
      SWAP
      DIV
      SWAP    ; Scale blue
      LOAD blue
      SWAP
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
.machine main globals 4 functions 4
    .global red 0
    .global green 1
    .global blue 2
    .global speed 3

    .func set_rgb index 0
      STORE red
      STORE blue
      STORE green
      PUSH 100
      STORE speed
      EXIT
    .end

    .func get_rgb_worker index 2
      .frame led_index 0
      .frame ticks 1
      PUSH 100
      STORE speed
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
      LOAD green
      LOAD blue
      RET 3
    .end

    .func get_rgb index 1
      PUSH 2
      CALL get_rgb_worker
      EXIT
    .end

    .func set_speed index 3
      LOAD speed
    .end

.end
`;



export const ASSEMBLER_PROGRAM = `
.machine main globals 3 functions 2

    .func set_rgb index 0
      STORE 0
      STORE 1
      STORE 2
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
      LOAD 1
      LOAD 2
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
        functionId: 1,
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
    assembly: ASSEMBLER_PROGRAM,
    controls: [
      new MachineControlDescriptor({
        id: "rainbow",
        label: "Pick Color",
        functionId: 1,
        type: "color_picker",
        min: 0,
        max: 1024,
        step: 1,
        defaultValue: 120,
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
