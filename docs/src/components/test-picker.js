import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';

const TEST_SUITES = [
  {
    name: 'Blargg CPU Instructions',
    profile: 'tests/blargg/blargg_cpu.toml',
    tests: [
      { name: '01-special', rom: 'cpu_instrs/individual/01-special.gb' },
      { name: '02-interrupts', rom: 'cpu_instrs/individual/02-interrupts.gb' },
      { name: '03-op sp,hl', rom: 'cpu_instrs/individual/03-op sp,hl.gb' },
      { name: '04-op r,imm', rom: 'cpu_instrs/individual/04-op r,imm.gb' },
      { name: '05-op rp', rom: 'cpu_instrs/individual/05-op rp.gb' },
      { name: '06-ld r,r', rom: 'cpu_instrs/individual/06-ld r,r.gb' },
      { name: '07-jr,jp,call,ret,rst', rom: 'cpu_instrs/individual/07-jr,jp,call,ret,rst.gb' },
      { name: '08-misc instrs', rom: 'cpu_instrs/individual/08-misc instrs.gb' },
      { name: '09-op r,r', rom: 'cpu_instrs/individual/09-op r,r.gb' },
      { name: '10-bit ops', rom: 'cpu_instrs/individual/10-bit ops.gb' },
      { name: '11-op a,(hl)', rom: 'cpu_instrs/individual/11-op a,(hl).gb' },
    ],
  },
];

const EMULATORS = ['gambatte', 'sameboy', 'mgba'];

function traceUrl(rom, emulator) {
  const base = rom.replace('.gb', '');
  return `tests/blargg/${base}_${emulator}.gbtrace.parquet`;
}

export class TestPicker extends LitElement {
  static styles = css`
    :host { display: block; }

    .picker {
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 16px 20px;
      max-width: 500px;
      width: 100%;
    }

    h3 {
      margin: 0 0 12px;
      font-size: 0.95rem;
      font-weight: 600;
    }

    .suite-name {
      font-size: 0.8rem;
      color: var(--text-muted);
      margin-bottom: 6px;
    }

    .row {
      display: flex;
      gap: 8px;
      align-items: center;
      margin-bottom: 4px;
    }

    select {
      flex: 1;
      padding: 6px 8px;
      background: var(--bg-secondary);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text);
      font-family: inherit;
      font-size: 0.85rem;
    }

    .emu-btns {
      display: flex;
      gap: 4px;
    }

    .emu-btn {
      padding: 5px 10px;
      background: var(--bg-secondary);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.8rem;
      font-family: inherit;
      transition: border-color 0.15s, color 0.15s;
    }
    .emu-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .emu-btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    .status {
      font-size: 0.8rem;
      margin-top: 8px;
    }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }

    .meta {
      display: flex;
      gap: 12px;
      margin-top: 10px;
      font-size: 0.75rem;
    }
    .meta a {
      color: var(--text-muted);
      text-decoration: none;
    }
    .meta a:hover {
      color: var(--accent);
    }
  `;

  static properties = {
    _selectedTest: { state: true },
    _loading: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this._selectedTest = 0;
    this._loading = null;
    this._error = null;
  }

  render() {
    const suite = TEST_SUITES[0];
    const test = suite.tests[this._selectedTest];

    return html`
      <div class="picker">
        <h3>Or load a test trace</h3>
        <div class="suite-name">${suite.name}</div>
        <div class="row">
          <select @change=${this._onTestChange}>
            ${suite.tests.map((t, i) => html`
              <option value=${i} ?selected=${i === this._selectedTest}>${t.name}</option>
            `)}
          </select>
        </div>
        <div class="emu-btns" style="margin-top: 8px">
          ${EMULATORS.map(emu => html`
            <button
              class="emu-btn"
              ?disabled=${this._loading !== null}
              @click=${() => this._load(test, emu)}
            >${emu}</button>
          `)}
        </div>
        <div class="meta">
          <a href="${suite.profile}" download>profile (.toml)</a>
          <a href="tests/blargg/${test.rom}" download>ROM (.gb)</a>
        </div>
        ${this._loading ? html`<p class="status loading">Loading ${this._loading}...</p>` : ''}
        ${this._error ? html`<p class="status error">${this._error}</p>` : ''}
      </div>
    `;
  }

  _onTestChange(e) {
    this._selectedTest = parseInt(e.target.value, 10);
    this._error = null;
  }

  async _load(test, emulator) {
    const url = traceUrl(test.rom, emulator);
    const filename = url.split('/').pop();
    this._loading = filename;
    this._error = null;

    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
      const buffer = await resp.arrayBuffer();
      const bytes = new Uint8Array(buffer);
      const store = await createTraceStore(bytes);
      this.dispatchEvent(new CustomEvent('trace-loaded', {
        detail: { store, filename },
        bubbles: true,
        composed: true,
      }));
    } catch (err) {
      this._error = `Failed to load: ${err.message || err}`;
    } finally {
      this._loading = null;
    }
  }
}

customElements.define('test-picker', TestPicker);
