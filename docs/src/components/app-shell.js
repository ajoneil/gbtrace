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
    _suite: { state: true },
    _testRom: { state: true },
    _testName: { state: true },
    _testInfo: { state: true },
    _store: { state: true },
    _storeB: { state: true },
    _nameA: { state: true },
    _nameB: { state: true },
    _header: { state: true },
    _highlightIndices: { state: true },
    _chartField: { state: true },
    _hoverIndex: { state: true },
    _diffStats: { state: true },
    _hiddenFields: { state: true },
    _downsampled: { state: true },
  };

  constructor() {
    super();
    this._suite = null;
    this._testRom = null;
    this._testInfo = null;
    this._testName = '';
    this._store = null;
    this._storeB = null;
    this._nameA = '';
    this._nameB = '';
    this._header = null;
    this._highlightIndices = null;
    this._chartField = null;
    this._hiddenFields = new Set();
    this._hoverIndex = null;
    this._diffStats = null;
    this._downsampled = false;
  }

  /** All fields from the trace header. */
  get _allFields() {
    return this._header?.fields || [];
  }

  /** Fields the user has selected (all minus hidden). Used for queries, stats, diff. */
  get _visibleFields() {
    return this._allFields.filter(f => !this._hiddenFields.has(f));
  }

  render() {
    return html`
      <div class="layout"
        @trace-loaded=${this._onTestPicked}
        @trace-selected=${this._onTraceSwitch}
        @trace-compare=${this._onTraceCompare}
        @trace-deselect-b=${this._exitCompare}
        @change-rom=${this._reset}
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
        @hidden-fields-changed=${this._onHiddenFieldsChanged}
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
        .testInfo=${this._testInfo}
        .activeA=${this._nameA}
        .activeB=${this._nameB}
        .allFields=${this._allFields}
        .hiddenFields=${this._hiddenFields}
        .triggerA=${this._header?.trigger || null}
        .triggerB=${this._storeB?.header()?.trigger || null}
        .downsampled=${this._downsampled}
      ></trace-selector>

      ${this._store
        ? (this._storeB ? this._renderCompare() : this._renderSingle())
        : ''
      }
    `;
  }

  _renderSingle() {
    const vf = this._visibleFields;
    return html`
      <div class="sections">
        <trace-query .store=${this._store} .fields=${vf}></trace-query>

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
          .fields=${this._allFields}
          .highlightIndices=${this._highlightIndices}
          .hiddenFields=${this._hiddenFields}
        ></trace-table>
      </div>
    `;
  }

  _renderCompare() {
    const vf = this._visibleFields;
    const countA = this._store.entryCount();
    const countB = this._storeB.entryCount();
    const minCount = Math.min(countA, countB);

    // Filter diff stats to only visible fields
    const stats = this._diffStats;
    const filteredStats = stats ? {
      ...stats,
      fields: stats.fields.filter(([name]) => !this._hiddenFields.has(name)),
    } : null;
    // Recompute match pct from filtered fields
    let matchPct = 100;
    if (filteredStats && filteredStats.total > 0) {
      // Count rows where ANY visible field differs
      let differing = filteredStats.differing;
      // If we've hidden some fields, the overall stats may over-count.
      // For accuracy, just show the field-level stats and skip overall %.
      // But as an approximation, use the max field diff count.
      if (this._hiddenFields.size > 0 && filteredStats.fields.length > 0) {
        const maxDiff = Math.max(...filteredStats.fields.map(([, c]) => c));
        differing = maxDiff;
      }
      matchPct = Math.round((1 - differing / filteredStats.total) * 1000) / 10;
    }

    return html`
      <div class="sections">
        ${filteredStats ? html`
          <div class="compare-stats">
            <span class="match-pct ${matchPct === 100 ? 'good' : matchPct > 90 ? 'partial' : 'bad'}">
              ${matchPct}% match
            </span>
            ${filteredStats.fields.length > 0 ? html`
              <span class="diff-fields">
                diffs in ${filteredStats.fields.map(([name, count]) => {
                  const pct = ((count / filteredStats.total) * 100).toFixed(1);
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
          .fields=${vf}
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
          .fields=${this._allFields}
          .highlightIndices=${this._highlightIndices}
          .hiddenFields=${this._hiddenFields}
        ></trace-diff-table>
      </div>
    `;
  }

  // --- Events ---

  _onTestPicked(e) {
    const { store, suite, testRom, emulator, testInfo } = e.detail;
    this._suite = suite;
    this._testRom = testRom;
    this._testName = testRom?.replace('.gb', '').split('/').pop() || '';
    this._testInfo = testInfo || null;
    this._setStoreA(store, emulator);
  }

  _onFileLoaded(e) {
    const { store, filename } = e.detail;
    this._suite = { base: '', profile: '' };
    this._testRom = null;
    this._testInfo = null;
    this._testName = filename;
    this._setStoreA(store, filename);
  }

  _onTraceSwitch(e) {
    const { store, name } = e.detail;
    this._setStoreA(store, name);
  }

  _onTraceCompare(e) {
    const { store, name } = e.detail;
    this._setStoreB(store, name);
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
    this._downsampled = false;
    // Don't reset _hiddenFields — persist across trace switches
  }

  _setStoreB(store, name) {
    if (this._storeB) this._storeB.free();

    // If triggers differ, collapse the T-cycle trace to instruction level
    const trigA = this._store?.header()?.trigger;
    const trigB = store.header()?.trigger;
    this._downsampled = false;
    if (trigA && trigB && trigA !== trigB) {
      try {
        if (trigA === 'tcycle') {
          const collapsed = this._store.collapseToInstructions();
          this._store.free();
          this._store = collapsed;
          this._header = collapsed.header();
          this._downsampled = true;
        } else if (trigB === 'tcycle') {
          const collapsed = store.collapseToInstructions();
          store.free();
          store = collapsed;
          this._downsampled = true;
        }
      } catch (err) {
        console.error('Failed to collapse T-cycle trace:', err);
      }
    }

    this._storeB = store;
    this._nameB = name;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._recomputeDiffStats();
  }

  _exitCompare() {
    if (this._storeB) this._storeB.free();
    this._storeB = null;
    this._nameB = '';
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    this._diffStats = null;
    this._downsampled = false;
  }

  _reset() {
    if (this._store) this._store.free();
    if (this._storeB) this._storeB.free();
    this._suite = null;
    this._testRom = null;
    this._testInfo = null;
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
    this._downsampled = false;
    this._hiddenFields = new Set();
  }

  _recomputeDiffStats() {
    if (!this._store || !this._storeB) {
      this._diffStats = null;
    this._downsampled = false;
      return;
    }
    try {
      this._diffStats = this._store.diffStats(this._storeB);
    } catch (err) {
      console.error('Failed to compute diff stats:', err);
      this._diffStats = null;
    this._downsampled = false;
    }
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

  _onHiddenFieldsChanged(e) {
    this._hiddenFields = e.detail.hiddenFields;
  }

}

customElements.define('app-shell', AppShell);
