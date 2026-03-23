import { LitElement, html, css } from 'lit';
import { prepareForDiffSync } from '../lib/wasm-bridge.js';
import './file-loader.js';
import './test-picker.js';
import './trace-selector.js';
import './trace-header.js';
import './trace-table.js';
import './trace-query.js';
import './trace-chart.js';
import './trace-diff-table.js';
import './trace-timeline.js';

export class AppShell extends LitElement {
  static styles = css`
    :host {
      display: block;
      min-height: 100vh;
    }
    .layout {
      margin: 0 auto;
      padding: 8px 24px 24px;
      width: 100%;
      box-sizing: border-box;
    }
    header {
      display: flex;
      align-items: center;
      gap: 12px;
      margin-bottom: 12px;
      padding-bottom: 8px;
      border-bottom: 1px solid var(--border);
    }
    header h1 {
      font-size: 1rem;
      font-weight: 600;
      cursor: pointer;
    }
    header h1 span {
      color: var(--text-muted);
      font-weight: 400;
      font-size: 0.8rem;
    }
    .wip-badge {
      font-size: 0.65rem;
      color: var(--yellow);
      border: 1px solid var(--yellow);
      border-radius: 4px;
      padding: 1px 6px;
      white-space: nowrap;
    }
    test-picker {
      display: flex;
      justify-content: center;
      margin-top: 16px;
    }
    .sections > * { margin-bottom: 12px; }
    .sections > trace-table,
    .sections > trace-diff-table {
      min-height: 500px;
      height: 70vh;
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
    _viewStart: { state: true },
    _viewEnd: { state: true },
    _frameBoundaries: { state: true },
    _frameBoundariesB: { state: true },
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
    this._viewStart = 0;
    this._viewEnd = 0;
    this._frameBoundaries = [];
    this._frameBoundariesB = [];
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
        @trace-selected=${this._onTraceSwitch}
        @trace-compare=${this._onTraceCompare}
        @trace-deselect-b=${this._exitCompare}
        @change-rom=${this._reset}
        @view-range-changed=${this._onViewRangeChanged}
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
        @hidden-fields-changed=${this._onHiddenFieldsChanged}
      >
        <header>
          <h1 @click=${this._reset}>gbtrace <span>Game Boy Trace Viewer</span></h1>
          <span class="wip-badge">🚧 under construction 🏗️</span>
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
      <test-picker @trace-loaded=${this._onTestPicked}></test-picker>
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

      ${this._store ? html`
        <trace-timeline
          .entryCount=${this._store.entryCount()}
          .entryCountB=${this._storeB?.entryCount() || 0}
          .frameBoundaries=${this._frameBoundaries}
          .frameBoundariesB=${this._frameBoundariesB}
          .viewStart=${this._viewStart}
          .viewEnd=${this._viewEnd}
          .compareMode=${!!this._storeB}
        ></trace-timeline>
      ` : ''}

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
        <trace-query .store=${this._store} .fields=${vf}
          .viewStart=${this._viewStart} .viewEnd=${this._viewEnd}
        ></trace-query>

        ${this._chartField ? html`
          <trace-chart
            .store=${this._store}
            .field=${this._chartField}
            .highlightIndices=${this._highlightIndices}
            .cursorIndex=${this._hoverIndex}
            .viewStart=${this._viewStart}
            .viewEnd=${this._viewEnd}
          ></trace-chart>
        ` : ''}

        <trace-table
          .store=${this._store}
          .viewStart=${this._viewStart}
          .viewEnd=${this._viewEnd}
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
          .viewStart=${this._viewStart}
          .viewEnd=${this._viewEnd}
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
            .viewStart=${this._viewStart}
            .viewEnd=${this._viewEnd}
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
          .viewStart=${this._viewStart}
          .viewEnd=${this._viewEnd}
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
    this._frameBoundaries = Array.from(store.frameBoundaries());
    this._frameBoundariesB = [];
    this._viewStart = 0;
    this._viewEnd = store.entryCount();
    // Don't reset _hiddenFields — persist across trace switches
  }

  _setStoreB(store, name) {
    if (this._storeB) this._storeB.free();

    // Use the library to handle collapse + alignment in one call
    const trigA = this._store?.header()?.trigger;
    const trigB = store.header()?.trigger;
    this._downsampled = false;

    try {
      const [prepA, prepB] = prepareForDiffSync(this._store, store);
      this._store = prepA;
      this._header = prepA.header();
      store = prepB;
      this._downsampled = (trigA !== trigB);
    } catch (err) {
      console.error('Failed to prepare traces for diff:', err);
    }

    this._storeB = store;
    this._nameB = name;
    this._highlightIndices = null;
    this._chartField = null;
    this._hoverIndex = null;
    // Recompute frame boundaries after diff preparation (stores may have changed)
    this._frameBoundaries = Array.from(this._store.frameBoundaries());
    this._frameBoundariesB = Array.from(store.frameBoundaries());
    this._viewStart = 0;
    this._viewEnd = this._store.entryCount();

    // Auto-hide fields missing from either trace
    const fieldsA = new Set(this._store.header()?.fields || []);
    const fieldsB = new Set(store.header()?.fields || []);
    const newHidden = new Set(this._hiddenFields);
    this._compareHiddenFields = new Set(); // track what we auto-hid
    for (const f of fieldsA) {
      if (!fieldsB.has(f)) {
        newHidden.add(f);
        this._compareHiddenFields.add(f);
      }
    }
    for (const f of fieldsB) {
      if (!fieldsA.has(f)) {
        newHidden.add(f);
        this._compareHiddenFields.add(f);
      }
    }
    this._hiddenFields = newHidden;

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
    // Restore fields that were auto-hidden for compare
    if (this._compareHiddenFields?.size) {
      const restored = new Set(this._hiddenFields);
      for (const f of this._compareHiddenFields) {
        restored.delete(f);
      }
      this._hiddenFields = restored;
      this._compareHiddenFields = null;
    }
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
      this._diffStats = this._store.diffStatsRange(this._storeB, this._viewStart, this._viewEnd);
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

  _onViewRangeChanged(e) {
    this._viewStart = e.detail.start;
    this._viewEnd = e.detail.end;
    this._recomputeDiffStats();
  }

}

customElements.define('app-shell', AppShell);
