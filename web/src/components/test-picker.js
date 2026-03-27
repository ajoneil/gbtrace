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
      { name: 'stat', filter: 'stat_' },
      { name: 'halt', filter: 'halt' },
    ],
  },
  {
    name: 'blargg',
    base: 'tests/blargg',
    profile: 'tests/blargg/profile.toml',
    manifest: 'tests/blargg/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'cpu instrs', filter: 'cpu_instrs__' },
      { name: 'instr timing', filter: 'instr_timing' },
    ],
  },
  {
    name: 'mooneye',
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
      { name: 'interrupts', filter: 'interrupts' },
      { name: 'halt', filter: 'halt' },
      { name: 'boot', filter: 'boot_' },
    ],
  },
  {
    name: 'gambatte-tests',
    base: 'tests/gambatte-tests',
    profile: 'tests/gambatte-tests/profile.toml',
    manifest: 'tests/gambatte-tests/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'sprites', filter: 'sprites__' },
      { name: 'palette m3', filter: 'dmgpalette' },
      { name: 'scx m3', filter: 'scx_during' },
      { name: 'div', filter: 'div__' },
      { name: 'halt', filter: 'halt__' },
      { name: 'stat irq', filter: 'miscmstatirq' },
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

const EMU_SHORT = { gateboy: 'GB', missingno: 'MN', gambatte: 'Ga', sameboy: 'SB', mgba: 'mG' };

function traceUrl(suite, test, emulator, status = 'pass') {
  return `${suite.base}/${test.name}_${emulator}_${status}.gbtrace`;
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
      max-width: 800px;
      width: 100%;
    }
    h3 { margin: 0 0 10px; font-size: 0.95rem; font-weight: 600; }

    .suite-tabs { display: flex; gap: 0; margin-bottom: 10px; border-bottom: 1px solid var(--border); }
    .suite-tab {
      padding: 6px 14px; background: none; border: none;
      border-bottom: 2px solid transparent; color: var(--text-muted);
      cursor: pointer; font-size: 0.8rem; font-family: inherit;
    }
    .suite-tab:hover { color: var(--text); }
    .suite-tab.active { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }

    .summary {
      display: flex; gap: 8px; margin-top: 12px; font-size: 0.72rem;
      font-family: var(--mono); flex-wrap: wrap;
    }
    .summary-emu {
      display: flex; flex-direction: column; gap: 2px;
      padding: 4px 8px; border-radius: 4px;
      background: var(--bg-secondary); border: 1px solid var(--border);
      min-width: 90px;
    }
    .summary-top { display: flex; align-items: center; gap: 4px; }
    .summary-emu .name { color: var(--text-muted); font-weight: 600; }
    .summary-emu .pass { color: #4caf50; }
    .summary-emu .fail { color: #f44336; }
    .summary-emu .missing { color: #ff9800; }
    .summary-bar {
      height: 4px; border-radius: 2px; display: flex; overflow: hidden;
      background: var(--bg);
    }
    .summary-bar .seg-pass { background: #4caf50; }
    .summary-bar .seg-fail { background: #f44336; }
    .summary-bar .seg-missing { background: #ff9800; opacity: 0.5; }

    .categories { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 8px; }
    .cat-chip {
      padding: 2px 8px; background: var(--bg); border: 1px solid var(--border);
      border-radius: 10px; color: var(--text-muted); cursor: pointer;
      font-size: 0.72rem; font-family: inherit;
    }
    .cat-chip:hover { border-color: var(--accent); color: var(--accent); }
    .cat-chip.active { background: var(--accent-subtle); border-color: var(--accent); color: var(--accent); }

    .search {
      width: 100%; padding: 5px 10px; background: var(--bg);
      border: 1px solid var(--border); border-radius: 6px; color: var(--text);
      font-family: var(--mono); font-size: 0.8rem; margin-bottom: 8px; box-sizing: border-box;
    }
    .search:focus { outline: none; border-color: var(--accent); }

    .test-count { font-size: 0.7rem; color: var(--text-muted); margin-bottom: 6px; }

    .test-list { max-height: 350px; overflow-y: auto; border: 1px solid var(--border); border-radius: 6px; }

    .test-row {
      display: flex; align-items: center; gap: 6px;
      padding: 4px 10px; border-bottom: 1px solid var(--bg);
      cursor: pointer; font-family: var(--mono); font-size: 0.75rem;
    }
    .test-row:last-child { border-bottom: none; }
    .test-row:hover { background: var(--bg-hover); }
    .test-row.selected { background: var(--accent-subtle); }

    .test-name { flex: 1; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .test-row:hover .test-name, .test-row.selected .test-name { color: var(--text); }

    .emu-dots { display: flex; gap: 2px; flex-shrink: 0; }
    .dot {
      width: 14px; height: 14px; border-radius: 3px; font-size: 0.55rem;
      display: flex; align-items: center; justify-content: center;
      font-weight: 700; cursor: pointer; border: 1px solid transparent;
    }
    .dot.pass { background: #1b3a1b; color: #4caf50; border-color: #2a5a2a; }
    .dot.fail { background: #3a1a1a; color: #f44336; border-color: #5a2a2a; }
    .dot.none { background: var(--bg-secondary); color: var(--text-muted); opacity: 0.3; }
    .dot:hover { opacity: 1; border-color: var(--accent); }

    .status { font-size: 0.8rem; margin-top: 8px; }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }
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
    this._selectedSuite = 0;
    this._selectedTest = -1;
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

  _getSuiteStats(suite) {
    if (!suite.tests) return {};
    const totalTests = suite.tests.length;
    const stats = {};
    for (const emu of EMULATORS) {
      let pass = 0, fail = 0;
      for (const test of suite.tests) {
        const s = test.emulators?.[emu];
        if (s === 'pass') pass++;
        else if (s === 'fail') fail++;
      }
      const missing = totalTests - pass - fail;
      if (pass > 0 || fail > 0) stats[emu] = { pass, fail, missing, total: totalTests };
    }
    return stats;
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

  render() {
    const suite = TEST_SUITES[this._selectedSuite];
    const tests = this._getTests(suite);
    const stats = this._getSuiteStats(suite);

    return html`
      <div class="picker">
        <h3>Test Suites</h3>

        <div class="suite-tabs">
          ${TEST_SUITES.map((s, i) => html`
            <button class="suite-tab ${i === this._selectedSuite ? 'active' : ''}"
              @click=${() => this._selectSuite(i)}
            >${s.name}${s.tests ? html` <span style="color:var(--text-muted);font-weight:400">(${s.tests.length})</span>` : ''}</button>
          `)}
        </div>

        ${suite.categories?.length ? html`
          <div class="categories">
            <span class="cat-chip ${!this._category ? 'active' : ''}"
              @click=${() => this._selectCategory('')}>all</span>
            ${suite.categories.map(c => html`
              <span class="cat-chip ${this._category === c.filter ? 'active' : ''}"
                @click=${() => this._selectCategory(c.filter)}>${c.name}</span>
            `)}
          </div>
        ` : ''}

        ${tests.length > 11 ? html`
          <input class="search" type="text" placeholder="filter tests..."
            .value=${this._search}
            @input=${e => { this._search = e.target.value; this._selectedTest = -1; }}>
        ` : ''}

        <div class="test-count">${tests.length} test${tests.length !== 1 ? 's' : ''}</div>

        <div class="test-list">
          ${tests.map((t, i) => {
            const emus = t.emulators || {};
            return html`
              <div class="test-row ${i === this._selectedTest ? 'selected' : ''}"
                @click=${() => this._selectTest(suite, tests, i)}>
                <span class="test-name">${t.name}</span>
                <div class="emu-dots">
                  ${EMULATORS.map(emu => {
                    const s = emus[emu];
                    const cls = s === 'pass' ? 'pass' : s === 'fail' ? 'fail' : 'none';
                    return html`
                      <span class="dot ${cls}"
                        title="${emu}: ${s || 'no trace'}"
                        @click=${e => { e.stopPropagation(); if (s) this._load(suite, t, emu, s); }}
                      >${EMU_SHORT[emu] || emu[0].toUpperCase()}</span>
                    `;
                  })}
                </div>
              </div>
            `;
          })}
        </div>

        ${this._loading ? html`<p class="status loading">Loading ${this._loading}...</p>` : ''}
        ${this._error ? html`<p class="status error">${this._error}</p>` : ''}

        ${Object.keys(stats).length > 0 ? html`
          <div class="summary">
            ${EMULATORS.filter(e => stats[e]).map(emu => {
              const s = stats[emu];
              const passPct = (s.pass / s.total * 100).toFixed(1);
              const failPct = (s.fail / s.total * 100).toFixed(1);
              const missPct = (s.missing / s.total * 100).toFixed(1);
              return html`
                <div class="summary-emu">
                  <div class="summary-top">
                    <span class="name">${emu}</span>
                    <span class="pass">${s.pass}</span>
                    ${s.fail > 0 ? html`<span class="fail">${s.fail}</span>` : ''}
                    ${s.missing > 0 ? html`<span class="missing">${s.missing}</span>` : ''}
                  </div>
                  <div class="summary-bar">
                    <div class="seg-pass" style="width:${passPct}%"></div>
                    <div class="seg-fail" style="width:${failPct}%"></div>
                    <div class="seg-missing" style="width:${missPct}%"></div>
                  </div>
                </div>
              `;
            })}
          </div>
        ` : ''}
      </div>
    `;
  }

  _selectSuite(i) {
    this._selectedSuite = i;
    this._selectedTest = -1;
    this._category = '';
    this._search = '';
    this._error = null;
  }

  _selectCategory(filter) {
    this._category = filter;
    this._selectedTest = -1;
    this._search = '';
  }

  _selectTest(suite, tests, index) {
    this._selectedTest = index;
    const test = tests[index];
    if (!test) return;

    const emus = test.emulators || {};
    const preferred = (suite.preferredEmu && emus[suite.preferredEmu])
      ? suite.preferredEmu
      : EMULATORS.find(e => emus[e]);
    if (preferred) {
      this._load(suite, test, preferred, emus[preferred] || 'pass');
    }
  }

  async loadFromHash(hash) {
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

    this._selectedSuite = TEST_SUITES.indexOf(suite);
    this._category = '';
    this._selectedTest = suite.tests.indexOf(test);
    this.requestUpdate();

    const emus = test.emulators || {};
    const emu = (suite.preferredEmu && emus[suite.preferredEmu])
      ? suite.preferredEmu
      : EMULATORS.find(e => emus[e]);
    if (emu) this._load(suite, test, emu, emus[emu] || 'pass');
  }

  async _load(suite, test, emulator, status = 'pass') {
    const url = traceUrl(suite, test, emulator, status);
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
