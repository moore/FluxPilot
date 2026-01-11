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

export const ASSEMBLER_PROGRAM = `
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

export const DEFAULT_MACHINE_RACK = [
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
];

class MachineCard extends HTMLElement {
  static get observedAttributes() {
    return ["title", "description", "action-label"];
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
    this.innerHTML = `
      <h3>${title}</h3>
      <p>${description}</p>
      <button class="machine-add" type="button">${actionLabel}</button>
    `;
  }
}

class MachineRack extends HTMLElement {
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
    return ["name", "meta", "speed", "hue", "gain"];
  }

  connectedCallback() {
    this.render();
  }

  attributeChangedCallback() {
    this.render();
  }

  render() {
    const name = this.getAttribute("name") ?? "Machine";
    const meta = this.getAttribute("meta") ?? "Machine";
    const speed = this.getAttribute("speed") ?? "1.0x";
    const hue = this.getAttribute("hue") ?? "+0";
    const gain = this.getAttribute("gain") ?? "60%";
    this.innerHTML = `
      <div class="track-machine">
        <div class="track-header">
          <strong>${name}</strong>
          <div class="track-actions">
            <div class="track-controls">
              <span class="chip">Speed ${speed}</span>
              <span class="chip">Hue ${hue}</span>
              <span class="chip">Gain ${gain}</span>
            </div>
            <button class="track-delete" type="button">Remove</button>
          </div>
        </div>
        <span class="chip">${meta}</span>
      </div>
    `;
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
    this.attachShadow({ mode: "open" });
    this.shadowRoot.innerHTML = `
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
    this.inputEl = this.shadowRoot.querySelector("input");
    this.labelEl = this.shadowRoot.querySelector("label");
    this.valueEl = this.shadowRoot.querySelector(".value");
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

export { ColorPicker };
