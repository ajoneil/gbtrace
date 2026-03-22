import { LitElement, html, css } from 'lit';
import './file-loader.js';
import './test-picker.js';
import './trace-selector.js';
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
    .compare-stats {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      font-size: 0.8rem;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
    }
    .compare-stats .match-pct {
      font-family: var(--mono);
      font-weight: 600;
    }
    .compare-stats .match-pct.good { color: var(--green); }
    .compare-stats .match-pct.partial { color: var(--yellow); }
    .compare-stats .match-pct.bad { color: var(--red); }
    .compare-stats .diff-fields {
      display: flex;
      gap: 6px;
      flex-wrap: wrap;
      font-size: 0.75rem;
      font-family: var(--mono);
      color: var(--text-muted);
    }
    .compare-stats .diff-field { color: var(--red); }
    .compare-stats .entries { color: var(--text-muted); margin-left: auto; }
  `;

  static properties = {
    // ROM context
    _suite: { state: true },
    _testRom: { state: true },
    _testName: { state: true },
    // Trace stores
    _store: { state: true },
    _storeB: { state: true },
    _nameA: { state: true },
    _nameB: { state: true },
    // Viewer state
    _header: { state: true },
    _highlightIndices: { state: true },
    _chartField: { state: true },
    _hoverIndex: { state: true },
    _diffStats: { state: true },
  };

  constructor() {
    super();
    this._suite = null;
    this._testRom = null;
    this._testName = '';
    this._store = null;
    this._storeB = null;
    this._nameA = '';
    this._nameB = '';
    this._header = null;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._diffStats = null;
  }

  render() {
    return html`
      <div class="layout"
        @trace-loaded=${this._onTestPicked}
        @trace-selected=${this._onTraceSelected}
        @trace-deselect-b=${this._exitCompare}
        @change-rom=${this._reset}
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
      >
        <header>
          <h1>gbtrace <span>Game Boy Trace Viewer</span></h1>
        </header>

        ${this._suite
          ? this._renderWithRom()
          : this._renderLanding()
        }
      </div>
    `;
  }

  _renderLanding() {
    return html`
      <file-loader @trace-loaded=${this._onFileLoaded}></file-loader>
      <test-picker></test-picker>
    `;
  }

  _renderWithRom() {
    return html`
      <trace-selector
        .suite=${this._suite}
        .testRom=${this._testRom}
        .testName=${this._testName}
        .activeA=${this._nameA}
        .activeB=${this._nameB}
      ></trace-selector>

      ${this._store
        ? (this._storeB ? this._renderCompare() : this._renderSingle())
        : ''
      }
    `;
  }

  _renderSingle() {
    const fields = this._header?.fields || [];
    return html`
      <div class="sections">
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
      <div class="sections">
        ${this._diffStats ? html`
          <div class="compare-stats">
            <span class="match-pct ${this._diffStats.match_pct === 100 ? 'good' : this._diffStats.match_pct > 90 ? 'partial' : 'bad'}">
              ${this._diffStats.match_pct}% match
            </span>
            ${this._diffStats.fields.length > 0 ? html`
              <span class="diff-fields">
                diffs in ${this._diffStats.fields.map(([name, count]) => {
                  const pct = ((count / this._diffStats.total) * 100).toFixed(1);
                  return html`<span class="diff-field">${name}<span style="color:var(--text-muted)">(${pct}%)</span></span>`;
                })}
              </span>
            ` : ''}
            <span class="entries">${minCount.toLocaleString()} entries</span>
          </div>
        ` : ''}

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

  // --- Events ---

  /** User picked a test from the test picker — enters ROM context */
  _onTestPicked(e) {
    const { store, suite, testRom, emulator } = e.detail;
    this._suite = suite;
    this._testRom = testRom;
    this._testName = testRom?.replace('.gb', '').split('/').pop() || '';
    this._setStoreA(store, emulator);
  }

  /** User dropped/browsed a file — no ROM context */
  _onFileLoaded(e) {
    const { store, filename } = e.detail;
    // No suite context — just show the trace
    this._suite = { base: '', profile: '' };
    this._testRom = null;
    this._testName = filename;
    this._setStoreA(store, filename);
  }

  /** Trace selected from the selector bar */
  _onTraceSelected(e) {
    const { store, name } = e.detail;
    if (!this._store) {
      // First trace — set as A
      this._setStoreA(store, name);
    } else {
      // Second trace — compare mode
      this._setStoreB(store, name);
    }
  }

  _setStoreA(store, name) {
    if (this._store) this._store.free();
    if (this._storeB) this._storeB.free();
    this._store = store;
    this._storeB = null;
    this._header = store.header();
    this._nameA = name || '';
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._diffStats = null;
  }

  _setStoreB(store, name) {
    if (this._storeB) this._storeB.free();
    this._storeB = store;
    this._nameB = name;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    try {
      this._diffStats = this._store.diffStats(store);
    } catch (err) {
      console.error('Failed to compute diff stats:', err);
      this._diffStats = null;
    }
  }

  _exitCompare() {
    if (this._storeB) this._storeB.free();
    this._storeB = null;
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._diffStats = null;
  }

  _reset() {
    if (this._store) this._store.free();
    if (this._storeB) this._storeB.free();
    this._suite = null;
    this._testRom = null;
    this._testName = '';
    this._store = null;
    this._storeB = null;
    this._nameA = '';
    this._nameB = '';
    this._header = null;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._diffStats = null;
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
}

customElements.define('app-shell', AppShell);
