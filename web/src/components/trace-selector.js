import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';
import { EMULATORS, traceUrl, romUrl } from './test-picker.js';

/** Field groups for toggling related columns together. */
const FIELD_GROUPS = [
  { name: 'cpu',       fields: ['pc', 'sp', 'a', 'f', 'b', 'c', 'd', 'e', 'h', 'l'] },
  { name: 'ppu',       fields: ['lcdc', 'stat', 'ly', 'lyc', 'scy', 'scx', 'wy', 'wx', 'bgp', 'obp0', 'obp1'] },
  { name: 'interrupt', fields: ['if_', 'ie', 'ime'] },
  { name: 'timer',     fields: ['div', 'tima', 'tma', 'tac'] },
  { name: 'serial',    fields: ['sb', 'sc'] },
  { name: 'ppu_int',   fields: [
    'pix', 'frame_num',
    'oam0_x', 'oam0_id', 'oam0_attr', 'oam1_x', 'oam1_id', 'oam1_attr',
    'oam2_x', 'oam2_id', 'oam2_attr', 'oam3_x', 'oam3_id', 'oam3_attr',
    'oam4_x', 'oam4_id', 'oam4_attr', 'oam5_x', 'oam5_id', 'oam5_attr',
    'oam6_x', 'oam6_id', 'oam6_attr', 'oam7_x', 'oam7_id', 'oam7_attr',
    'oam8_x', 'oam8_id', 'oam8_attr', 'oam9_x', 'oam9_id', 'oam9_attr',
    'bgw_fifo_a', 'bgw_fifo_b', 'spr_fifo_a', 'spr_fifo_b',
    'mask_pipe', 'pal_pipe',
    'tfetch_state', 'sfetch_state', 'tile_temp_a', 'tile_temp_b',
    'pix_count', 'sprite_count', 'scan_count', 'rendering', 'win_mode',
  ]},
  { name: 'test',      fields: ['test_result', 'test_expect', 'test_pass'] },
];

export class TraceSelector extends LitElement {
  static styles = css`
    :host { display: block; }
    .bar {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 8px 12px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      font-size: 0.8rem;
      flex-wrap: wrap;
    }
    .rom-name {
      font-family: var(--mono);
      font-weight: 600;
      color: var(--text);
      margin-right: 4px;
    }
    .sep { color: var(--border); margin: 0 2px; }
    .label { font-size: 0.7rem; color: var(--text-muted); }
    .trace-btn {
      padding: 4px 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.78rem;
      font-family: inherit;
      transition: all 0.15s;
      max-width: 140px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .trace-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .trace-btn.active {
      background: rgba(88,166,255,0.1);
      border-color: #58a6ff;
      color: #58a6ff;
      font-weight: 600;
    }
    .trace-btn.compare {
      background: rgba(210,153,34,0.1);
      border-color: #d29922;
      color: #d29922;
      font-weight: 600;
    }
    .trace-btn:disabled { opacity: 0.5; cursor: not-allowed; }
    .trace-btn .status-dot {
      display: inline-block;
      width: 6px; height: 6px;
      border-radius: 50%;
      margin-right: 4px;
    }
    .trace-btn .status-dot.pass { background: var(--green); }
    .trace-btn .status-dot.fail { background: var(--red); }
    .add-btn {
      padding: 4px 8px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.78rem;
      font-family: inherit;
      transition: all 0.15s;
    }
    .add-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    input[type="file"] { display: none; }
    .status { font-size: 0.75rem; }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }
    .fields-row {
      display: flex;
      flex-wrap: wrap;
      gap: 3px;
      align-items: center;
      width: 100%;
      padding-top: 6px;
      border-top: 1px solid var(--border);
      margin-top: 6px;
    }
    .ft-label {
      font-size: 0.7rem;
      color: var(--text-muted);
      margin-right: 2px;
    }
    .ft-chip {
      padding: 1px 7px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 8px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.7rem;
      font-family: var(--mono);
      user-select: none;
      transition: all 0.1s;
    }
    .ft-chip:hover { border-color: var(--accent); color: var(--accent); }
    .ft-chip.on {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }
    .ft-group {
      padding: 1px 8px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.68rem;
      font-family: var(--sans);
      font-weight: 700;
      user-select: none;
      transition: all 0.1s;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }
    .ft-group:hover { border-color: var(--accent); color: var(--accent); }
    .ft-group.on {
      background: var(--bg-hover);
      border-color: var(--text-muted);
      color: var(--text);
    }
    .ft-sep {
      width: 1px;
      height: 16px;
      background: var(--border);
      margin: 0 2px;
    }
    .downloads {
      display: flex;
      gap: 8px;
      font-size: 0.7rem;
      margin-left: auto;
    }
    .downloads a {
      color: var(--text-muted);
      text-decoration: none;
    }
    .downloads a:hover { color: var(--accent); }
    .trigger-badge {
      font-size: 0.65rem;
      color: var(--text-muted);
      font-family: var(--mono);
      padding: 1px 5px;
      border: 1px solid var(--border);
      border-radius: 6px;
      margin-left: 4px;
    }
    .trigger-badge.downsampled {
      color: var(--yellow);
      border-color: var(--yellow);
    }
  `;

  static properties = {
    suite: { type: Object },
    testRom: { type: String },
    testName: { type: String },
    testInfo: { type: Object },
    activeA: { type: String },
    activeB: { type: String },
    allFields: { type: Array },
    hiddenFields: { type: Object },
    excludedFields: { type: Object },
    triggerA: { type: String },
    triggerB: { type: String },
    downsampled: { type: Boolean },
    hasPixels: { type: Boolean },
    pixelsActive: { type: Boolean },
    _uploads: { state: true },
    _loading: { state: true },
    _error: { state: true },
    _hiddenGroups: { state: true },
  };

  constructor() {
    super();
    this.suite = null;
    this.testRom = null;
    this.testName = '';
    this.testInfo = null;
    this.activeA = null;
    this.activeB = null;
    this.allFields = [];
    this.hiddenFields = new Set();
    this.excludedFields = null;
    this.triggerA = null;
    this.triggerB = null;
    this.downsampled = false;
    this.hasPixels = false;
    this.pixelsActive = false;
    this._uploads = []; // { name, store }
    this._loading = null;
    this._error = null;
    this._hiddenGroups = new Set(); // group names that are toggled off
  }

  /** All available trace names (library emulators with traces + uploads). */
  get _allTraces() {
    const emus = this.testInfo?.emulators || {};
    const lib = this.suite
      ? EMULATORS.filter(e => emus[e]).map(e => ({ name: e, type: 'lib', status: emus[e] }))
      : [];
    const ups = this._uploads.map(u => ({ name: u.name, type: 'upload', status: null }));
    return [...lib, ...ups];
  }

  render() {
    const traces = this._allTraces;
    const hasActive = !!this.activeA;

    return html`
      <div class="bar">
        <span class="rom-name">${this.testName || this.testRom || 'trace'}</span>
        ${this.triggerA ? html`<span class="trigger-badge ${this.downsampled ? 'downsampled' : ''}">${this.downsampled ? 'instruction (downsampled)' : this.triggerA}</span>` : ''}
        <span class="sep">|</span>

        ${traces.map(t => {
          const isA = this.activeA === t.name;
          const isB = this.activeB === t.name;
          const cls = isA ? 'trace-btn active' : isB ? 'trace-btn compare' : 'trace-btn';
          return html`
            <button
              class="${cls}"
              ?disabled=${this._loading !== null}
              @click=${() => this._onTraceClick(t.name)}
              title=${t.name}
            >${t.status ? html`<span class="status-dot ${t.status}"></span>` : ''}${t.name}</button>
          `;
        })}

        ${hasActive ? html`
          <span class="sep">|</span>
          <span class="label">compare</span>
          ${traces.filter(t => t.name !== this.activeA).map(t => {
            const isB = this.activeB === t.name;
            return html`
              <button
                class="trace-btn ${isB ? 'compare' : ''}"
                ?disabled=${this._loading !== null}
                @click=${() => this._onCompareClick(t.name)}
                title=${t.name}
              >${t.status ? html`<span class="status-dot ${t.status}"></span>` : ''}${t.name}</button>
            `;
          })}
        ` : ''}

        ${this._loading ? html`<span class="status loading">loading ${this._loading}...</span>` : ''}
        ${this._error ? html`<span class="status error">${this._error}</span>` : ''}

        <button
          class="add-btn"
          style="margin-left:auto;"
          ?disabled=${this._loading !== null}
          @click=${this._clickUpload}
          title="upload a trace file"
        >+ upload</button>
        <input type="file" accept=".gbtrace,.gz,.parquet" @change=${this._onFileChange}>

        ${this.allFields.length ? html`
          <div class="fields-row">
            <span class="ft-label">columns</span>
            ${this._renderFieldGroups()}
            ${this.suite?.profile || this.testRom ? html`
              <span class="downloads">
                ${this.suite?.profile ? html`<a href="${this.suite.profile}" download>profile</a>` : ''}
                ${this.testRom ? html`<a href="${romUrl(this.suite, this.testRom)}" download>ROM</a>` : ''}
              </span>
            ` : ''}
          </div>
        ` : ''}
      </div>
    `;
  }

  _renderFieldGroups() {
    const excluded = this.excludedFields || new Set();
    const allFields = (this.allFields || []).filter(f => !excluded.has(f));
    const hidden = this.hiddenFields || new Set();
    const grouped = new Set();
    const parts = [];

    // Render each group that has fields in this trace
    for (const group of FIELD_GROUPS) {
      const present = group.fields.filter(f => allFields.includes(f));
      if (present.length === 0) continue;
      present.forEach(f => grouped.add(f));

      const anyVisible = present.some(f => !hidden.has(f));
      const groupHidden = this._hiddenGroups.has(group.name) || !anyVisible;

      parts.push(html`
        <span class="ft-group ${anyVisible ? 'on' : ''}"
          @click=${() => this._toggleGroup(group.name)}
        >${group.name}</span>
      `);

      // Show individual field chips only when group is expanded
      if (!groupHidden) {
        for (const f of present) {
          parts.push(html`
            <span class="ft-chip ${hidden.has(f) ? '' : 'on'}"
              @click=${() => this._toggleField(f)}
            >${f}</span>
          `);
        }
      }

      parts.push(html`<span class="ft-sep"></span>`);
    }

    // Ungrouped fields (e.g. custom memory addresses)
    const ungrouped = allFields.filter(f => !grouped.has(f));
    for (const f of ungrouped) {
      parts.push(html`
        <span class="ft-chip ${hidden.has(f) ? '' : 'on'}"
          @click=${() => this._toggleField(f)}
        >${f}</span>
      `);
    }

    return parts;
  }

  _toggleGroup(groupName) {
    const group = FIELD_GROUPS.find(g => g.name === groupName);
    if (!group) return;

    const present = group.fields.filter(f => (this.allFields || []).includes(f));
    const newHiddenGroups = new Set(this._hiddenGroups);
    const newHidden = new Set(this.hiddenFields || []);

    if (this._hiddenGroups.has(groupName)) {
      // Show group — remove group fields from hidden
      // (restores them all; individual toggles within the group are preserved
      // because we only added them when hiding the group)
      newHiddenGroups.delete(groupName);
      for (const f of present) {
        newHidden.delete(f);
      }
    } else {
      // Hide group — add all group fields to hidden
      newHiddenGroups.add(groupName);
      for (const f of present) {
        newHidden.add(f);
      }
    }

    this._hiddenGroups = newHiddenGroups;
    this.dispatchEvent(new CustomEvent('hidden-fields-changed', {
      detail: { hiddenFields: newHidden },
      bubbles: true, composed: true,
    }));
  }

  _toggleField(f) {
    const s = new Set(this.hiddenFields || []);
    if (s.has(f)) s.delete(f); else s.add(f);
    this.dispatchEvent(new CustomEvent('hidden-fields-changed', {
      detail: { hiddenFields: s },
      bubbles: true, composed: true,
    }));
  }

  _togglePixels() {
    this.dispatchEvent(new CustomEvent('field-selected', {
      detail: { field: this.pixelsActive ? null : '__pixels__' },
      bubbles: true, composed: true,
    }));
  }

  _onTraceClick(name) {
    if (this.activeA === name) return;
    // Clicking any trace loads it as primary (single view)
    this._activateTrace(name, 'trace-selected');
  }

  _onCompareClick(name) {
    if (this.activeB === name) {
      // Clicking active B deselects it
      this.dispatchEvent(new CustomEvent('trace-deselect-b', {
        bubbles: true, composed: true,
      }));
      return;
    }
    this._activateTrace(name, 'trace-compare');
  }

  async _activateTrace(name, eventName) {
    // Check if it's an already-loaded upload
    const upload = this._uploads.find(u => u.name === name);
    if (upload) {
      // Re-create store from saved bytes (stores can only be used once)
      this._loading = name;
      this._error = null;
      try {
        const store = await createTraceStore(new Uint8Array(upload.bytes));
        if (this.suite && this.testRom) {
          try {
            const rResp = await fetch(romUrl(this.suite, this.testRom));
            if (rResp.ok) store.loadRom(new Uint8Array(await rResp.arrayBuffer()));
          } catch (_) {}
        }
        this.dispatchEvent(new CustomEvent(eventName, {
          detail: { store, name },
          bubbles: true, composed: true,
        }));
      } catch (err) {
        this._error = `${err.message || err}`;
      } finally {
        this._loading = null;
      }
      return;
    }

    // Library emulator — look up status from testInfo
    const emus = this.testInfo?.emulators || {};
    const status = emus[name] || 'pass';
    await this._loadEmu(name, eventName, status);
  }

  async _loadEmu(emu, eventName, status = 'pass') {
    if (!this.suite || !this.testRom) return;
    const url = traceUrl(this.suite, this.testRom, emu, status);
    this._loading = emu;
    this._error = null;

    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
      const store = await createTraceStore(new Uint8Array(await resp.arrayBuffer()));

      try {
        const rResp = await fetch(romUrl(this.suite, this.testRom));
        if (rResp.ok) store.loadRom(new Uint8Array(await rResp.arrayBuffer()));
      } catch (_) {}

      this.dispatchEvent(new CustomEvent(eventName, {
        detail: { store, name: emu },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `${err.message || err}`;
    } finally {
      this._loading = null;
    }
  }

  _clickUpload() {
    this.renderRoot.querySelector('input[type="file"]').click();
  }

  async _onFileChange(e) {
    const file = e.target.files?.[0];
    if (!file) return;

    this._loading = file.name;
    this._error = null;

    try {
      const bytes = await file.arrayBuffer();
      const store = await createTraceStore(new Uint8Array(bytes));

      // Save bytes so we can re-create the store when switching
      const name = file.name;
      this._uploads = [...this._uploads, { name, bytes: new Uint8Array(bytes) }];

      this.dispatchEvent(new CustomEvent('trace-selected', {
        detail: { store, name },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `${err.message || err}`;
    } finally {
      this._loading = null;
      e.target.value = '';
    }
  }

}

customElements.define('trace-selector', TraceSelector);
