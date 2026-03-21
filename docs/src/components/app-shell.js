import { LitElement, html, css } from 'lit';
import './file-loader.js';
import './test-picker.js';
import './trace-header.js';
import './trace-table.js';
import './trace-query.js';
import './trace-chart.js';
import './trace-diff-table.js';

export class AppShell extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      height: 100vh;
    }
    .layout {
      max-width: 1400px;
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
    test-picker {
      display: flex;
      justify-content: center;
      margin-top: 16px;
    }
    .sections {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    .sections > * { margin-bottom: 12px; }
    .sections > trace-table,
    .sections > trace-diff-table {
      flex: 1;
      min-height: 0;
    }
    .compare-header {
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 8px 12px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      font-size: 0.8rem;
    }
    .compare-header .label { color: var(--text-muted); }
    .compare-header .name-a { color: #58a6ff; font-weight: 600; font-family: var(--mono); }
    .compare-header .vs { color: var(--text-muted); }
    .compare-header .name-b { color: #d29922; font-weight: 600; font-family: var(--mono); }
    .compare-header .entries { color: var(--text-muted); margin-left: auto; }
  `;

  static properties = {
    _store: { state: true },
    _storeB: { state: true },
    _header: { state: true },
    _filename: { state: true },
    _nameA: { state: true },
    _nameB: { state: true },
    _highlightIndices: { state: true },
    _chartField: { state: true },
    _hoverIndex: { state: true },
  };

  constructor() {
    super();
    this._store = null;
    this._storeB = null;
    this._header = null;
    this._filename = null;
    this._nameA = '';
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
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

        ${this._store
          ? (this._storeB ? this._renderCompare() : this._renderViewer())
          : html`
            <file-loader @trace-loaded=${this._onTraceLoaded}></file-loader>
            <test-picker
              @trace-loaded=${this._onTraceLoaded}
              @trace-compare=${this._onTraceCompare}
            ></test-picker>
          `
        }
      </div>
    `;
  }

  _renderViewer() {
    const fields = this._header?.fields || [];
    return html`
      <div class="sections"
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
      >
        <trace-header
          .header=${this._header}
          .entryCount=${this._store.entryCount()}
          .filename=${this._filename}
        ></trace-header>

        <trace-query .store=${this._store} .fields=${fields}></trace-query>

        ${this._chartField ? html`
          <trace-chart
            .store=${this._store}
            .field=${this._chartField}
            .highlightIndices=${this._highlightIndices}
            .cursorIndex=${this._hoverIndex}
          ></trace-chart>
        ` : ''}

        <trace-table
          .store=${this._store}
          .fields=${fields}
          .highlightIndices=${this._highlightIndices}
        ></trace-table>
      </div>
    `;
  }

  _renderCompare() {
    const fields = this._header?.fields || [];
    const countA = this._store.entryCount();
    const countB = this._storeB.entryCount();
    const minCount = Math.min(countA, countB);

    return html`
      <div class="sections"
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
      >
        <div class="compare-header">
          <span class="label">Comparing</span>
          <span class="name-a">${this._nameA}</span>
          <span class="vs">vs</span>
          <span class="name-b">${this._nameB}</span>
          <span class="entries">${minCount.toLocaleString()} entries (sequence aligned)</span>
        </div>

        <trace-query
          .store=${this._store}
          .storeB=${this._storeB}
          .fields=${fields}
          .compareMode=${true}
        ></trace-query>

        ${this._chartField ? html`
          <trace-chart
            .store=${this._store}
            .storeB=${this._storeB}
            .nameA=${this._nameA}
            .nameB=${this._nameB}
            .field=${this._chartField}
            .highlightIndices=${this._highlightIndices}
            .cursorIndex=${this._hoverIndex}
          ></trace-chart>
        ` : ''}

        <trace-diff-table
          .storeA=${this._store}
          .storeB=${this._storeB}
          .nameA=${this._nameA}
          .nameB=${this._nameB}
          .fields=${fields}
          .highlightIndices=${this._highlightIndices}
        ></trace-diff-table>
      </div>
    `;
  }

  _onTraceLoaded(e) {
    const { store, filename } = e.detail;
    this._store = store;
    this._storeB = null;
    this._header = store.header();
    this._filename = filename;
    this._nameA = '';
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
  }

  _onTraceCompare(e) {
    const { storeA, storeB, nameA, nameB } = e.detail;
    this._store = storeA;
    this._storeB = storeB;
    this._header = storeA.header();
    this._filename = `${nameA} vs ${nameB}`;
    this._nameA = nameA;
    this._nameB = nameB;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
  }

  _onHighlightChanged(e) {
    this._highlightIndices = e.detail.indices;
  }

  _onJumpToIndex(e) {
    const table = this.renderRoot?.querySelector('trace-table') ||
                  this.renderRoot?.querySelector('trace-diff-table');
    if (table) table.scrollToIndex(e.detail.index);
  }

  _onFieldSelected(e) {
    this._chartField = e.detail.field;
  }

  _onHoverIndex(e) {
    this._hoverIndex = e.detail.index;
  }

  _reset() {
    if (this._store) this._store.free();
    if (this._storeB) this._storeB.free();
    this._store = null;
    this._storeB = null;
    this._header = null;
    this._filename = null;
    this._nameA = '';
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
  }
}

customElements.define('app-shell', AppShell);
