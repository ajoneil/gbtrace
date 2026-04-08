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
      { name: 'dmg sound', filter: 'dmg_sound__' },
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
    name: 'mealybug-tearoom',
    base: 'tests/mealybug-tearoom',
    profile: 'tests/mealybug-tearoom/profile.toml',
    manifest: 'tests/mealybug-tearoom/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
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
  {
    name: 'age',
    base: 'tests/age',
    profile: 'tests/age/profile.toml',
    manifest: 'tests/age/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'halt', filter: 'halt' },
      { name: 'stat', filter: 'stat' },
      { name: 'oam', filter: 'oam' },
      { name: 'vram', filter: 'vram' },
      { name: 'm3 bg', filter: 'm3-bg' },
    ],
  },
  {
    name: 'mooneye-wilbertpol',
    base: 'tests/mooneye-wilbertpol',
    profile: 'tests/mooneye-wilbertpol/profile.toml',
    manifest: 'tests/mooneye-wilbertpol/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'gpu', filter: 'acceptance__gpu' },
      { name: 'timer', filter: 'acceptance__timer' },
      { name: 'mbc', filter: 'emulator-only' },
    ],
  },
  {
    name: 'samesuite',
    base: 'tests/samesuite',
    profile: 'tests/samesuite/profile.toml',
    manifest: 'tests/samesuite/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [
      { name: 'apu', filter: 'apu' },
      { name: 'interrupt', filter: 'interrupt' },
    ],
  },
  {
    name: 'scribbltests',
    base: 'tests/scribbltests',
    profile: 'tests/scribbltests/profile.toml',
    manifest: 'tests/scribbltests/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
  {
    name: 'bully',
    base: 'tests/bully',
    profile: 'tests/bully/profile.toml',
    manifest: 'tests/bully/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
  {
    name: 'little-things',
    base: 'tests/little-things',
    profile: 'tests/little-things/profile.toml',
    manifest: 'tests/little-things/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
  {
    name: 'mbc3-tester',
    base: 'tests/mbc3-tester',
    profile: 'tests/mbc3-tester/profile.toml',
    manifest: 'tests/mbc3-tester/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
  {
    name: 'strikethrough',
    base: 'tests/strikethrough',
    profile: 'tests/strikethrough/profile.toml',
    manifest: 'tests/strikethrough/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
  {
    name: 'turtle-tests',
    base: 'tests/turtle-tests',
    profile: 'tests/turtle-tests/profile.toml',
    manifest: 'tests/turtle-tests/manifest.json',
    preferredEmu: 'gateboy',
    tests: null,
    categories: [],
  },
];

const EMULATORS = ['gateboy', 'missingno', 'gambatte', 'sameboy', 'mgba'];

const EMU_SHORT = { gateboy: 'GB', missingno: 'MN', gambatte: 'Ga', sameboy: 'SB', mgba: 'mG' };

/** Known uppercase abbreviations in test names. */
const ABBREVIATIONS = new Set([
  'dma', 'oam', 'ppu', 'cpu', 'gpu', 'apu', 'lcd', 'div', 'ei', 'di',
  'ie', 'if', 'ly', 'scx', 'scy', 'wx', 'wy', 'hl', 'sp', 'bc', 'de',
  'af', 'pc', 'mbc', 'vram', 'hram', 'stat', 'lcdc', 'bgp', 'obp',
  'rst', 'jp', 'jr', 'ret', 'ld', 'rp', 'imm',
  'dmg', 'irq', 'intr', 'int', 'gs',
  'hblank', 'vblank', 'oamscan', 'lcdon',
]);

/**
 * Convert an underscore-heavy test filename into a readable display name.
 * e.g. "hblank_int_di_timing_b" → "HBlank Int DI Timing B"
 *      "dma_0x1000" → "DMA 0x1000"
 *      "cpu_instrs__02-interrupts" → "CPU Instrs / 02 Interrupts"
 */
function humanizeTestName(name) {
  // Double-underscore is a path separator in some suites (blargg)
  let s = name.replace(/__/g, ' / ').replace(/_/g, ' ').replace(/-/g, ' ');
  // Capitalize words, respecting abbreviations and hex literals
  s = s.replace(/\b([a-zA-Z0-9]+)\b/g, (match) => {
    if (/^0[xXbBdD]/.test(match)) return match;         // hex/binary literal
    if (/^\d/.test(match)) return match;                 // number
    if (ABBREVIATIONS.has(match.toLowerCase())) return match.toUpperCase();
    return match.charAt(0).toUpperCase() + match.slice(1);
  });
  return s;
}

export { humanizeTestName };

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

    .suite-row {
      display: flex; align-items: center; gap: 8px; margin-bottom: 10px; flex-wrap: wrap;
    }
    .suite-select {
      padding: 5px 10px; background: var(--bg); border: 1px solid var(--border);
      border-radius: 6px; color: var(--text); font-family: inherit;
      font-size: 0.8rem; cursor: pointer; min-width: 160px;
    }
    .suite-select:focus { outline: none; border-color: var(--accent); }
    .suite-count {
      font-size: 0.72rem; color: var(--text-muted); font-family: var(--mono);
    }

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
      cursor: pointer; font-size: 0.78rem;
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
      tests = tests.filter(t =>
        t.name.toLowerCase().includes(q) ||
        humanizeTestName(t.name).toLowerCase().includes(q)
      );
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

        <div class="suite-row">
          <select class="suite-select" @change=${e => this._selectSuite(parseInt(e.target.value, 10))}>
            ${TEST_SUITES.map((s, i) => html`
              <option value=${i} ?selected=${i === this._selectedSuite}
              >${s.name}${s.tests ? ` (${s.tests.length})` : ''}</option>
            `)}
          </select>

          ${suite.categories?.length ? html`
            <span class="cat-chip ${!this._category ? 'active' : ''}"
              @click=${() => this._selectCategory('')}>all</span>
            ${suite.categories.map(c => html`
              <span class="cat-chip ${this._category === c.filter ? 'active' : ''}"
                @click=${() => this._selectCategory(c.filter)}>${c.name}</span>
            `)}
          ` : ''}
        </div>

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
                <span class="test-name" title="${t.name}">${humanizeTestName(t.name)}</span>
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
