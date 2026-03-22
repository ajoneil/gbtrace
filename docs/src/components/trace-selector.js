import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';
import { EMULATORS, traceUrl, romUrl } from './test-picker.js';

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
    .change-btn {
      padding: 4px 10px;
      background: none;
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.75rem;
      font-family: inherit;
      margin-left: auto;
    }
    .change-btn:hover {
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
    triggerA: { type: String },
    triggerB: { type: String },
    downsampled: { type: Boolean },
    _uploads: { state: true },
    _loading: { state: true },
    _error: { state: true },
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
    this.triggerA = null;
    this.triggerB = null;
    this.downsampled = false;
    this._uploads = []; // { name, store }
    this._loading = null;
    this._error = null;
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

        <button
          class="add-btn"
          ?disabled=${this._loading !== null}
          @click=${this._clickUpload}
          title="upload a trace file"
        >+ upload</button>
        <input type="file" accept=".gbtrace,.gz,.parquet" @change=${this._onFileChange}>

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

        <button class="change-btn" @click=${this._changeRom}>change ROM</button>

        ${this.allFields.length ? html`
          <div class="fields-row">
            <span class="ft-label">columns</span>
            ${this.allFields.map(f => html`
              <span
                class="ft-chip ${this.hiddenFields?.has(f) ? '' : 'on'}"
                @click=${() => this._toggleField(f)}
              >${f}</span>
            `)}
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

  _toggleField(f) {
    const s = new Set(this.hiddenFields || []);
    if (s.has(f)) s.delete(f); else s.add(f);
    this.dispatchEvent(new CustomEvent('hidden-fields-changed', {
      detail: { hiddenFields: s },
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

  _changeRom() {
    this.dispatchEvent(new CustomEvent('change-rom', {
      bubbles: true, composed: true,
    }));
  }
}

customElements.define('trace-selector', TraceSelector);
