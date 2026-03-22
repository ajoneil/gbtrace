import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';

const TEST_SUITES = [
  {
    name: 'gbmicrotest',
    base: 'tests/gbmicrotest',
    profile: 'tests/gbmicrotest/gbmicrotest.toml',
    tests: null, // loaded from manifest
    categories: [
      { name: 'poweron', filter: 'poweron_' },
      { name: 'timer', filter: 'timer_' },
      { name: 'ppu', filter: 'ppu_' },
      { name: 'oam', filter: 'oam_' },
      { name: 'dma', filter: 'dma_' },
      { name: 'lcd', filter: 'lcdon_' },
      { name: 'vram', filter: 'vram_' },
      { name: 'sprite', filter: 'sprite' },
      { name: 'window', filter: 'win' },
      { name: 'interrupt', filter: 'int_' },
      { name: 'line', filter: 'line_' },
      { name: 'stat', filter: 'stat_' },
      { name: 'hblank', filter: 'hblank' },
      { name: 'vblank', filter: 'vblank' },
      { name: 'lyc', filter: 'lyc' },
      { name: 'halt', filter: 'halt' },
    ],
  },
  {
    name: 'Blargg CPU',
    base: 'tests/blargg',
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

function traceUrl(suite, rom, emulator) {
  const base = rom.replace('.gb', '');
  return `${suite.base}/${base}_${emulator}.gbtrace.parquet`;
}

function romUrl(suite, rom) {
  return `${suite.base}/${rom}`;
}

export { TEST_SUITES, EMULATORS, traceUrl, romUrl };

export class TestPicker extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      align-items: center;
    }
    .picker {
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 16px 20px;
      max-width: 600px;
      width: 100%;
    }
    h3 {
      margin: 0 0 10px;
      font-size: 0.95rem;
      font-weight: 600;
    }

    /* Suite tabs */
    .suite-tabs {
      display: flex;
      gap: 0;
      margin-bottom: 10px;
      border-bottom: 1px solid var(--border);
    }
    .suite-tab {
      padding: 6px 14px;
      background: none;
      border: none;
      border-bottom: 2px solid transparent;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.8rem;
      font-family: inherit;
    }
    .suite-tab:hover { color: var(--text); }
    .suite-tab.active {
      color: var(--accent);
      border-bottom-color: var(--accent);
      font-weight: 600;
    }

    /* Category chips */
    .categories {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
      margin-bottom: 8px;
    }
    .cat-chip {
      padding: 2px 8px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 10px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.72rem;
      font-family: inherit;
      transition: all 0.15s;
    }
    .cat-chip:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .cat-chip.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }

    /* Search */
    .search {
      width: 100%;
      padding: 5px 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text);
      font-family: var(--mono);
      font-size: 0.8rem;
      margin-bottom: 8px;
      box-sizing: border-box;
    }
    .search:focus {
      outline: none;
      border-color: var(--accent);
    }
    .search::placeholder { color: var(--text-muted); }

    /* Test list */
    .test-list {
      max-height: 200px;
      overflow-y: auto;
      border: 1px solid var(--border);
      border-radius: 6px;
      margin-bottom: 8px;
    }
    .test-item {
      padding: 4px 10px;
      font-family: var(--mono);
      font-size: 0.78rem;
      cursor: pointer;
      border-bottom: 1px solid var(--bg);
      color: var(--text-muted);
    }
    .test-item:last-child { border-bottom: none; }
    .test-item:hover { background: var(--bg-hover); color: var(--text); }
    .test-item.selected {
      background: var(--accent-subtle);
      color: var(--accent);
    }
    .test-count {
      font-size: 0.7rem;
      color: var(--text-muted);
      margin-bottom: 6px;
    }

    /* Emulator buttons */
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

    .status { font-size: 0.8rem; margin-top: 8px; }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }

    .meta {
      display: flex;
      gap: 12px;
      margin-top: 8px;
      font-size: 0.75rem;
    }
    .meta a { color: var(--text-muted); text-decoration: none; }
    .meta a:hover { color: var(--accent); }
  `;

  static properties = {
    _selectedSuite: { state: true },
    _selectedTest: { state: true },
    _microTests: { state: true },
    _category: { state: true },
    _search: { state: true },
    _loading: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this._selectedSuite = 0; // gbmicrotest first
    this._selectedTest = 0;
    this._microTests = null;
    this._category = '';
    this._search = '';
    this._loading = null;
    this._error = null;
    this._loadMicroManifest();
  }

  async _loadMicroManifest() {
    try {
      const resp = await fetch('tests/gbmicrotest/manifest.json');
      if (resp.ok) this._microTests = await resp.json();
    } catch (_) {}
  }

  render() {
    const suite = TEST_SUITES[this._selectedSuite];
    const tests = this._getTests(suite);
    const test = tests?.[this._selectedTest];

    return html`
      <div class="picker">
        <h3>Load a test trace</h3>

        <div class="suite-tabs">
          ${TEST_SUITES.map((s, i) => html`
            <button
              class="suite-tab ${i === this._selectedSuite ? 'active' : ''}"
              @click=${() => this._selectSuite(i)}
            >${s.name}</button>
          `)}
        </div>

        ${suite.categories ? html`
          <div class="categories">
            <span
              class="cat-chip ${!this._category ? 'active' : ''}"
              @click=${() => this._selectCategory('')}
            >all</span>
            ${suite.categories.map(c => html`
              <span
                class="cat-chip ${this._category === c.filter ? 'active' : ''}"
                @click=${() => this._selectCategory(c.filter)}
              >${c.name}</span>
            `)}
          </div>
        ` : ''}

        ${tests.length > 11 ? html`
          <input
            class="search"
            type="text"
            placeholder="filter tests..."
            .value=${this._search}
            @input=${e => { this._search = e.target.value; this._selectedTest = 0; }}
          >
        ` : ''}

        <div class="test-count">${tests.length} test${tests.length !== 1 ? 's' : ''}</div>

        <div class="test-list">
          ${tests.map((t, i) => html`
            <div
              class="test-item ${i === this._selectedTest ? 'selected' : ''}"
              @click=${() => this._selectedTest = i}
            >${t.name}</div>
          `)}
        </div>

        ${test ? html`
          <div class="emu-btns">
            ${EMULATORS.map(emu => html`
              <button
                class="emu-btn"
                ?disabled=${this._loading !== null}
                @click=${() => this._load(suite, test, emu)}
              >${emu}</button>
            `)}
          </div>
          <div class="meta">
            <a href="${suite.profile}" download>profile</a>
            <a href="${romUrl(suite, test.rom)}" download>ROM</a>
          </div>
        ` : ''}

        ${this._loading ? html`<p class="status loading">Loading ${this._loading}...</p>` : ''}
        ${this._error ? html`<p class="status error">${this._error}</p>` : ''}
      </div>
    `;
  }

  _getTests(suite) {
    if (suite.tests) {
      if (!this._search) return suite.tests;
      const q = this._search.toLowerCase();
      return suite.tests.filter(t => t.name.toLowerCase().includes(q));
    }
    if (!this._microTests) return [];
    let names = this._microTests;
    if (this._category) {
      names = names.filter(n => n.startsWith(this._category));
    }
    if (this._search) {
      const q = this._search.toLowerCase();
      names = names.filter(n => n.toLowerCase().includes(q));
    }
    return names.map(n => ({ name: n, rom: `${n}.gb` }));
  }

  _selectSuite(i) {
    this._selectedSuite = i;
    this._selectedTest = 0;
    this._category = '';
    this._search = '';
    this._error = null;
  }

  _selectCategory(filter) {
    this._category = filter;
    this._selectedTest = 0;
    this._search = '';
    this._error = null;
  }

  async _load(suite, test, emulator) {
    const url = traceUrl(suite, test.rom, emulator);
    const filename = url.split('/').pop();
    this._loading = filename;
    this._error = null;

    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
      const store = await createTraceStore(new Uint8Array(await resp.arrayBuffer()));

      try {
        const ru = romUrl(suite, test.rom);
        const romResp = await fetch(ru);
        if (romResp.ok) store.loadRom(new Uint8Array(await romResp.arrayBuffer()));
      } catch (_) {}

      this.dispatchEvent(new CustomEvent('trace-loaded', {
        detail: { store, filename, suite, testRom: test.rom, emulator },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `Failed to load: ${err.message || err}`;
    } finally {
      this._loading = null;
    }
  }
}

customElements.define('test-picker', TestPicker);
