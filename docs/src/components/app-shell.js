import { LitElement, html, css } from 'lit';
import './file-loader.js';
import './trace-header.js';
import './trace-table.js';
import './trace-query.js';

export class AppShell extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      height: 100vh;
    }
    .layout {
      max-width: 1200px;
      margin: 0 auto;
      padding: 24px;
      width: 100%;
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: 24px;
      padding-bottom: 16px;
      border-bottom: 1px solid var(--border);
    }
    header h1 {
      font-size: 1.3rem;
      font-weight: 600;
    }
    header h1 span {
      color: var(--text-muted);
      font-weight: 400;
      font-size: 0.9rem;
    }
    .new-trace {
      padding: 6px 12px;
      background: none;
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.8rem;
    }
    .new-trace:hover { border-color: var(--accent); color: var(--accent); }
    .sections {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    .sections > * { margin-bottom: 12px; }
    .sections > trace-table {
      flex: 1;
      min-height: 0;
    }
  `;

  static properties = {
    _store: { state: true },
    _header: { state: true },
    _filename: { state: true },
    _highlightIndices: { state: true },
  };

  constructor() {
    super();
    this._store = null;
    this._header = null;
    this._filename = null;
    this._highlightIndices = null;
  }

  render() {
    return html`
      <div class="layout">
        <header>
          <h1>gbtrace <span>Game Boy Trace Viewer</span></h1>
          ${this._store ? html`
            <button class="new-trace" @click=${this._reset}>Load another</button>
          ` : ''}
        </header>

        ${this._store ? this._renderViewer() : html`
          <file-loader @trace-loaded=${this._onTraceLoaded}></file-loader>
        `}
      </div>
    `;
  }

  _renderViewer() {
    const fields = this._header?.fields || [];
    return html`
      <div class="sections"
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
      >
        <trace-header
          .header=${this._header}
          .entryCount=${this._store.entryCount()}
          .filename=${this._filename}
        ></trace-header>

        <trace-query .store=${this._store} .fields=${fields}></trace-query>

        <trace-table
          .store=${this._store}
          .fields=${fields}
          .highlightIndices=${this._highlightIndices}
        ></trace-table>
      </div>
    `;
  }

  _onTraceLoaded(e) {
    const { store, filename } = e.detail;
    this._store = store;
    this._header = store.header();
    this._filename = filename;
    this._highlightIndices = null;
  }

  _onHighlightChanged(e) {
    this._highlightIndices = e.detail.indices;
  }

  _onJumpToIndex(e) {
    const table = this.renderRoot?.querySelector('trace-table');
    if (table) table.scrollToIndex(e.detail.index);
  }

  _reset() {
    if (this._store) {
      this._store.free();
    }
    this._store = null;
    this._header = null;
    this._filename = null;
    this._highlightIndices = null;
  }
}

customElements.define('app-shell', AppShell);
