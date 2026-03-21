import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';
import { EMULATORS, traceUrl } from './test-picker.js';

export class CompareBar extends LitElement {
  static styles = css`
    :host { display: block; }
    .bar {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      font-size: 0.8rem;
    }
    .label {
      color: var(--text-muted);
      white-space: nowrap;
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
      transition: border-color 0.15s, color 0.15s;
    }
    .emu-btn:hover {
      border-color: var(--yellow);
      color: var(--yellow);
    }
    .emu-btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
    .sep {
      color: var(--border);
      margin: 0 4px;
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
      transition: border-color 0.15s, color 0.15s;
    }
    .upload-btn:hover {
      border-color: var(--yellow);
      color: var(--yellow);
    }
    input[type="file"] { display: none; }
    .status {
      font-size: 0.75rem;
      margin-left: 8px;
    }
    .status.loading { color: var(--accent); }
    .status.error { color: var(--red); }
  `;

  static properties = {
    testRom: { type: String },
    emulator: { type: String },
    _loading: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this.testRom = null;
    this.emulator = null;
    this._loading = null;
    this._error = null;
  }

  render() {
    const otherEmus = this.testRom
      ? EMULATORS.filter(e => e !== this.emulator)
      : [];

    return html`
      <div class="bar">
        <span class="label">Compare with</span>
        ${otherEmus.map(emu => html`
          <button
            class="emu-btn"
            ?disabled=${this._loading !== null}
            @click=${() => this._loadEmulator(emu)}
          >${emu}</button>
        `)}
        ${otherEmus.length > 0 ? html`<span class="sep">|</span>` : ''}
        <button
          class="upload-btn"
          ?disabled=${this._loading !== null}
          @click=${this._clickUpload}
        >upload file</button>
        <input type="file" accept=".gbtrace,.gz,.parquet" @change=${this._onFileChange}>
        ${this._loading ? html`<span class="status loading">Loading ${this._loading}...</span>` : ''}
        ${this._error ? html`<span class="status error">${this._error}</span>` : ''}
      </div>
    `;
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
      const buffer = await file.arrayBuffer();
      const store = await createTraceStore(new Uint8Array(buffer));
      this.dispatchEvent(new CustomEvent('compare-loaded', {
        detail: { store, name: file.name },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `Failed: ${err.message || err}`;
    } finally {
      this._loading = null;
    }
  }

  async _loadEmulator(emu) {
    if (!this.testRom) return;
    const url = traceUrl(this.testRom, emu);
    this._loading = emu;
    this._error = null;

    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
      const buffer = await resp.arrayBuffer();
      const store = await createTraceStore(new Uint8Array(buffer));
      this.dispatchEvent(new CustomEvent('compare-loaded', {
        detail: { store, name: emu },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `Failed: ${err.message || err}`;
    } finally {
      this._loading = null;
    }
  }
}

customElements.define('compare-bar', CompareBar);
