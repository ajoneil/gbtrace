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
    .sep {
      color: var(--border);
      margin: 0 2px;
    }
    .label {
      font-size: 0.7rem;
      color: var(--text-muted);
    }
    .emu-btn {
      padding: 4px 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.78rem;
      font-family: inherit;
      transition: all 0.15s;
    }
    .emu-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .emu-btn.active {
      background: rgba(88,166,255,0.1);
      border-color: #58a6ff;
      color: #58a6ff;
      font-weight: 600;
    }
    .emu-btn.compare {
      background: rgba(210,153,34,0.1);
      border-color: #d29922;
      color: #d29922;
      font-weight: 600;
    }
    .emu-btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
    .upload-btn {
      padding: 4px 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.78rem;
      font-family: inherit;
      transition: all 0.15s;
    }
    .upload-btn:hover {
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
    .status {
      font-size: 0.75rem;
    }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }
  `;

  static properties = {
    suite: { type: Object },
    testRom: { type: String },
    testName: { type: String },
    activeA: { type: String },
    activeB: { type: String },
    _loading: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this.suite = null;
    this.testRom = null;
    this.testName = '';
    this.activeA = null;
    this.activeB = null;
    this._loading = null;
    this._error = null;
  }

  render() {
    const others = this.suite
      ? EMULATORS.filter(e => e !== this.activeA)
      : [];

    return html`
      <div class="bar">
        <span class="rom-name">${this.testName || this.testRom || 'trace'}</span>
        <span class="sep">|</span>

        ${this.suite ? EMULATORS.map(emu => {
          const isA = this.activeA === emu;
          const isB = this.activeB === emu;
          const cls = isA ? 'emu-btn active' : isB ? 'emu-btn compare' : 'emu-btn';
          return html`
            <button
              class="${cls}"
              ?disabled=${this._loading !== null}
              @click=${() => this._onEmuClick(emu)}
            >${emu}</button>
          `;
        }) : ''}
        <button
          class="upload-btn"
          ?disabled=${this._loading !== null}
          @click=${() => this._clickUpload(false)}
        >upload</button>

        ${this.activeA ? html`
          <span class="sep">|</span>
          <span class="label">compare</span>
          ${others.map(emu => {
            const isB = this.activeB === emu;
            return html`
              <button
                class="emu-btn ${isB ? 'compare' : ''}"
                ?disabled=${this._loading !== null}
                @click=${() => this._onCompareClick(emu)}
              >${emu}</button>
            `;
          })}
          <button
            class="upload-btn"
            ?disabled=${this._loading !== null}
            @click=${() => this._clickUpload(true)}
          >upload</button>
        ` : ''}

        <input type="file" id="upload-main" accept=".gbtrace,.gz,.parquet" @change=${(e) => this._onFileChange(e, false)}>
        <input type="file" id="upload-compare" accept=".gbtrace,.gz,.parquet" @change=${(e) => this._onFileChange(e, true)}>

        ${this._loading ? html`<span class="status loading">loading ${this._loading}...</span>` : ''}
        ${this._error ? html`<span class="status error">${this._error}</span>` : ''}

        <button class="change-btn" @click=${this._changeRom}>change ROM</button>
      </div>
    `;
  }

  _onEmuClick(emu) {
    // Clicking the compare trace switches it to single view
    if (this.activeB === emu) {
      this._loadEmu(emu, 'trace-selected');
      return;
    }
    // Clicking any emulator (including current A) loads as single view
    if (this.activeA === emu) return; // already active
    this._loadEmu(emu, 'trace-selected');
  }

  _onCompareClick(emu) {
    // Clicking active B deselects it
    if (this.activeB === emu) {
      this.dispatchEvent(new CustomEvent('trace-deselect-b', {
        bubbles: true, composed: true,
      }));
      return;
    }
    // Load as comparison
    this._loadEmu(emu, 'trace-compare');
  }

  async _loadEmu(emu, eventName) {
    if (!this.suite || !this.testRom) return;
    const url = traceUrl(this.suite, this.testRom, emu);
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

  _clickUpload(asCompare) {
    const id = asCompare ? '#upload-compare' : '#upload-main';
    this.renderRoot.querySelector(id).click();
  }

  async _onFileChange(e, asCompare) {
    const file = e.target.files?.[0];
    if (!file) return;
    this._loading = file.name;
    this._error = null;

    try {
      const store = await createTraceStore(new Uint8Array(await file.arrayBuffer()));
      const eventName = asCompare ? 'trace-compare' : 'trace-selected';
      this.dispatchEvent(new CustomEvent(eventName, {
        detail: { store, name: file.name },
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
