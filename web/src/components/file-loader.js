import { LitElement, html, css } from 'lit';
import { createTraceStore } from '../lib/wasm-bridge.js';

export class FileLoader extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      min-height: 300px;
    }
    .dropzone {
      border: 2px dashed var(--border);
      border-radius: 12px;
      padding: 48px;
      text-align: center;
      cursor: pointer;
      transition: border-color 0.2s, background 0.2s;
      max-width: 500px;
      width: 100%;
    }
    .dropzone:hover, .dropzone.dragover {
      border-color: var(--accent);
      background: var(--accent-subtle);
    }
    .dropzone h2 {
      margin-bottom: 8px;
      font-size: 1.2rem;
    }
    .dropzone p {
      color: var(--text-muted);
      font-size: 0.9rem;
    }
    .loading {
      color: var(--accent);
      margin-top: 16px;
    }
    .error {
      color: var(--red);
      margin-top: 16px;
    }
    input[type="file"] { display: none; }
  `;

  static properties = {
    _loading: { state: true },
    _error: { state: true },
    _dragover: { state: true },
  };

  constructor() {
    super();
    this._loading = false;
    this._error = null;
    this._dragover = false;
  }

  render() {
    return html`
      <div
        class="dropzone ${this._dragover ? 'dragover' : ''}"
        @click=${this._onClick}
        @dragover=${this._onDragOver}
        @dragleave=${this._onDragLeave}
        @drop=${this._onDrop}
      >
        <h2>Load a trace file</h2>
        <p>Drop a .gbtrace, .gbtrace.jsonl, or .gbtrace.jsonl.gz file here, or click to browse</p>
      </div>
      <input type="file" accept=".gbtrace,.jsonl,.gz" @change=${this._onFileChange}>
      ${this._loading ? html`<p class="loading">Loading...</p>` : ''}
      ${this._error ? html`<p class="error">${this._error}</p>` : ''}
    `;
  }

  _onClick() {
    this.renderRoot.querySelector('input[type="file"]').click();
  }

  _onDragOver(e) {
    e.preventDefault();
    this._dragover = true;
  }

  _onDragLeave() {
    this._dragover = false;
  }

  async _onDrop(e) {
    e.preventDefault();
    this._dragover = false;
    const file = e.dataTransfer?.files?.[0];
    if (file) await this._loadFile(file);
  }

  async _onFileChange(e) {
    const file = e.target.files?.[0];
    if (file) await this._loadFile(file);
  }

  async _loadFile(file) {
    this._loading = true;
    this._error = null;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = new Uint8Array(buffer);
      const store = await createTraceStore(bytes);
      this.dispatchEvent(new CustomEvent('trace-loaded', {
        detail: { store, filename: file.name },
        bubbles: true, composed: true,
      }));
    } catch (err) {
      this._error = `Failed to load: ${err.message || err}`;
    } finally {
      this._loading = false;
    }
  }
}

customElements.define('file-loader', FileLoader);
