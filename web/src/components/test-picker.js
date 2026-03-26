import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';

const TEST_SUITES = [
  {
    name: 'gbmicrotest',
    base: 'tests/gbmicrotest',
    profile: 'tests/gbmicrotest/profile.toml',
    manifest: 'tests/gbmicrotest/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
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
    name: 'Blargg',
    base: 'tests/blargg',
    profile: 'tests/blargg/profile.toml',
    manifest: 'tests/blargg/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'cpu instrs', filter: 'cpu_instrs/' },
      { name: 'instr timing', filter: 'instr_timing' },
    ],
  },
  {
    name: 'Mooneye',
    base: 'tests/mooneye',
    profile: 'tests/mooneye/profile.toml',
    manifest: 'tests/mooneye/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'timer', filter: 'timer' },
      { name: 'ppu', filter: 'ppu' },
      { name: 'oam dma', filter: 'oam_dma' },
      { name: 'bits', filter: 'bits' },
      { name: 'instr', filter: 'instr/' },
      { name: 'interrupts', filter: 'interrupts' },
      { name: 'serial', filter: 'serial' },
      { name: 'halt', filter: 'halt' },
      { name: 'boot', filter: 'boot_' },
      { name: 'timing', filter: 'timing' },
      { name: 'call/ret', filter: 'call' },
      { name: 'ei/di', filter: 'ei_' },
    ],
  },
  {
    name: 'dmg-acid2',
    base: 'tests/dmg-acid2',
    profile: 'tests/dmg-acid2/profile.toml',
    manifest: 'tests/dmg-acid2/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
];

const EMULATORS = ['gateboy', 'missingno', 'gambatte', 'sameboy', 'mgba'];

function traceUrl(suite, rom, emulator, status = 'pass') {
  const base = rom.replace('.gb', '');
  return `${suite.base}/${base}_${emulator}_${status}.gbtrace`;
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
    _category: { state: true },
    _search: { state: true },
    _loading: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this._selectedSuite = 0; // gbmicrotest first
    this._selectedTest = 0;
    this._category = '';
    this._search = '';
    this._loading = null;
    this._error = null;
    this._loadManifests();
  }

  async _loadManifests() {
    for (const suite of TEST_SUITES) {
      if (!suite.manifest) continue;
      try {
        const resp = await fetch(suite.manifest);
        if (resp.ok) {
          suite.tests = await resp.json();
          this.requestUpdate();
        }
      } catch (_) {}
    }
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
              @click=${() => this._selectTest(suite, tests, i)}
            >${t.name}</div>
          `)}
        </div>


        ${this._loading ? html`<p class="status loading">Loading ${this._loading}...</p>` : ''}
        ${this._error ? html`<p class="status error">${this._error}</p>` : ''}
      </div>
    `;
  }

  _getTests(suite) {
    if (!suite.tests) return [];
    let tests = suite.tests;
    if (this._category) {
      tests = tests.filter(t =>
        t.name.includes(this._category) ||
        (t.rom && t.rom.includes(this._category))
      );
    }
    if (this._search) {
      const q = this._search.toLowerCase();
      tests = tests.filter(t => t.name.toLowerCase().includes(q));
    }
    return tests;
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

  _selectTest(suite, tests, index) {
    this._selectedTest = index;
    const test = tests[index];
    if (!test) return;

    // Auto-load the preferred emulator for this suite, falling back to first available
    const emus = test.emulators || {};
    const preferred = (suite.preferredEmu && emus[suite.preferredEmu])
      ? suite.preferredEmu
      : EMULATORS.find(e => emus[e]);
    if (preferred) {
      this._load(suite, test, preferred, emus[preferred] || 'pass');
    }
  }

  /** Deep-link: parse "suiteName/testPath" and auto-load the test. */
  async loadFromHash(hash) {
    // Wait for manifests to load
    await this._loadManifests();

    const slashIdx = hash.indexOf('/');
    if (slashIdx < 0) return;
    const suiteName = hash.slice(0, slashIdx).toLowerCase();
    const testPath = hash.slice(slashIdx + 1);

    const suite = TEST_SUITES.find(s => s.name.toLowerCase() === suiteName);
    if (!suite || !suite.tests) return;

    const test = suite.tests.find(t =>
      t.rom?.replace('.gb', '') === testPath || t.name === testPath
    );
    if (!test) return;

    // Select the suite and test in the UI
    this._selectedSuite = TEST_SUITES.indexOf(suite);
    this._category = '';
    this._selectedTest = suite.tests.indexOf(test);
    this.requestUpdate();

    // Load with preferred emulator
    const emus = test.emulators || {};
    const emu = (suite.preferredEmu && emus[suite.preferredEmu])
      ? suite.preferredEmu
      : EMULATORS.find(e => emus[e]);
    if (emu) {
      this._load(suite, test, emu, emus[emu] || 'pass');
    }
  }

  async _load(suite, test, emulator, status = 'pass') {
    const url = traceUrl(suite, test.rom, emulator, status);
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
        detail: { store, filename, suite, testRom: test.rom, emulator, testInfo: test },
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
