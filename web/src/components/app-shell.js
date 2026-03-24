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
import './pixel-display.js';
import './ppu-sprite-table.js';
import './ppu-fifo-visualizer.js';

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
    .scrubber-row {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 4px 0;
    }
    .scrubber-row input[type="range"] {
      flex: 1;
      max-width: 400px;
    }
    .scrubber-row .scrub-info {
      font-size: 0.7rem;
      color: var(--text-muted);
      font-family: var(--mono);
      white-space: nowrap;
    }
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
    _currentIndex: { state: true },
    _diffStats: { state: true },
    _hiddenFields: { state: true },
    _downsampled: { state: true },
    _viewStart: { state: true },
    _viewEnd: { state: true },
    _frameBoundaries: { state: true },
    _frameBoundariesB: { state: true },
    _syncMode: { state: true },
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
    this._currentIndex = null;
    this._diffStats = null;
    this._downsampled = false;
    this._viewStart = 0;
    this._viewEnd = 0;
    this._frameBoundaries = [];
    this._frameBoundariesB = [];
    this._syncMode = 'pc';
  }

  /** All fields from the trace header. */
  get _allFields() {
    return this._header?.fields || [];
  }

  /** The effective cursor index: hover takes priority, falls back to current. */
  get _effectiveIndex() {
    return this._hoverIndex ?? this._currentIndex;
  }

  /** True if the trace includes PPU internal fields. */
  get _hasPpuInternals() {
    return this._allFields.includes('oam0_x');
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
        @sync-changed=${this._onSyncChanged}
        @highlight-changed=${this._onHighlightChanged}
        @jump-to-index=${this._onJumpToIndex}
        @field-selected=${this._onFieldSelected}
        @hover-index=${this._onHoverIndex}
        @current-index=${this._onCurrentIndex}
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
        .excludedFields=${this._compareHiddenFields || null}
        .triggerA=${this._header?.trigger || null}
        .triggerB=${this._storeB?.header()?.trigger || null}
        .downsampled=${this._downsampled}
        .hasPixels=${this._store?.hasPixels() || false}
        .pixelsActive=${this._chartField === '__pixels__'}
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
          .syncMode=${this._syncMode}
        ></trace-timeline>
      ` : ''}

      ${this._store
        ? (this._storeB ? this._renderCompare() : this._renderSingle())
        : ''
      }
    `;
  }

  /** Get the frame range for the current view. */
  _getCurrentFrameRange() {
    const bounds = this._frameBoundaries || [];
    let frameIdx = 0;
    for (let i = 0; i < bounds.length; i++) {
      if (bounds[i] <= this._viewStart) frameIdx = i;
      else break;
    }
    const start = bounds[frameIdx] || 0;
    const end = frameIdx + 1 < bounds.length ? bounds[frameIdx + 1] : (this._store?.entryCount() || 0);
    return { start, end };
  }

  _renderSingle() {
    const vf = this._visibleFields;
    const isTcycle = this._store?.isTcyclePixels() || false;
    const { start: frameStart, end: frameEnd } = this._getCurrentFrameRange();
    return html`
      <div class="sections">
        <trace-query .store=${this._store} .fields=${vf}
          .viewStart=${this._viewStart} .viewEnd=${this._viewEnd}
        ></trace-query>

        ${this._chartField === '__pixels__' && isTcycle ? html`
          <div class="scrubber-row">
            <input type="range"
              min=${frameStart} max=${frameEnd}
              .value=${String(this._currentIndex ?? frameStart)}
              @input=${this._onScrub}>
            <span class="scrub-info">entry ${this._currentIndex ?? frameStart} / ${frameEnd}</span>
          </div>
        ` : ''}

        ${this._chartField === '__pixels__' ? html`
          <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:flex-start;">
            <pixel-display
              .store=${this._store}
              .frameBoundaries=${this._frameBoundaries}
              .viewStart=${this._viewStart}
              .tcyclePixels=${isTcycle}
              .currentIndex=${this._effectiveIndex}
            ></pixel-display>
            ${this._hasPpuInternals ? html`
              <div style="display:flex;flex-direction:column;gap:8px;min-width:200px;">
                <ppu-sprite-table
                  .store=${this._store}
                  .cursorIndex=${this._effectiveIndex ?? this._viewStart}
                ></ppu-sprite-table>
                <ppu-fifo-visualizer
                  .store=${this._store}
                  .cursorIndex=${this._effectiveIndex ?? this._viewStart}
                ></ppu-fifo-visualizer>
              </div>
            ` : ''}
          </div>
        ` : this._chartField ? html`
          <trace-chart
            .store=${this._store}
            .field=${this._chartField}
            .highlightIndices=${this._highlightIndices}
            .cursorIndex=${this._effectiveIndex}
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
          .tcyclePixels=${this._store?.isTcyclePixels() || false}
          .currentIndex=${this._currentIndex}
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
    // Recompute match pct from visible (non-hidden) diff fields only
    let matchPct = 100;
    if (filteredStats && filteredStats.total > 0 && filteredStats.fields.length > 0) {
      const maxDiff = Math.max(...filteredStats.fields.map(([, c]) => c));
      matchPct = Math.round((1 - maxDiff / filteredStats.total) * 1000) / 10;
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

        ${this._chartField === '__pixels__' ? html`
          <pixel-display
            .store=${this._store}
            .storeB=${this._storeB}
            .nameA=${this._nameA}
            .nameB=${this._nameB}
            .frameBoundaries=${this._frameBoundaries}
            .frameBoundariesB=${this._frameBoundariesB}
            .viewStart=${this._viewStart}
          ></pixel-display>
        ` : this._chartField ? html`
          <trace-chart
            .store=${this._store}
            .storeB=${this._storeB}
            .nameA=${this._nameA}
            .nameB=${this._nameB}
            .field=${this._chartField}
            .highlightIndices=${this._highlightIndices}
            .cursorIndex=${this._effectiveIndex}
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
    this._currentIndex = 0;
    this._diffStats = null;
    this._downsampled = false;
    this._frameBoundaries = Array.from(store.frameBoundaries());
    this._frameBoundariesB = [];
    this._viewStart = 0;
    this._viewEnd = store.entryCount();
    // Auto-hide PPU internal fields from the table (they're shown in
    // dedicated PPU widgets instead). Don't touch other hidden fields.
    const ppuFields = [
      'oam0_x','oam0_id','oam0_attr','oam1_x','oam1_id','oam1_attr',
      'oam2_x','oam2_id','oam2_attr','oam3_x','oam3_id','oam3_attr',
      'oam4_x','oam4_id','oam4_attr','oam5_x','oam5_id','oam5_attr',
      'oam6_x','oam6_id','oam6_attr','oam7_x','oam7_id','oam7_attr',
      'oam8_x','oam8_id','oam8_attr','oam9_x','oam9_id','oam9_attr',
      'bgw_fifo_a','bgw_fifo_b','spr_fifo_a','spr_fifo_b',
      'mask_pipe','pal_pipe',
      'tfetch_state','sfetch_state','tile_temp_a','tile_temp_b',
      'pix_count','sprite_count','scan_count','rendering','win_mode',
    ];
    const fields = this._allFields;
    if (fields.some(f => ppuFields.includes(f))) {
      const h = new Set(this._hiddenFields || []);
      for (const f of ppuFields) {
        if (fields.includes(f)) h.add(f);
      }
      this._hiddenFields = h;
    }
  }

  _setStoreB(store, name) {
    if (this._storeB) this._storeB.free();

    // Use the library to handle collapse + alignment in one call
    const trigA = this._store?.header()?.trigger;
    const trigB = store.header()?.trigger;
    this._downsampled = false;

    // Default to frame sync when both traces have pixel data
    if (this._syncMode === 'pc' && this._store?.hasPixels() && store.hasPixels()) {
      this._syncMode = 'ly=0';
    }

    try {
      const [prepA, prepB] = prepareForDiffSync(this._store, store, this._syncMode);
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
    this._currentIndex = e.detail.index;
    this._scrollTableToCurrent();
  }

  _onFieldSelected(e) {
    const prev = this._chartField;
    this._chartField = e.detail.field;
    // When entering pixel compare mode, switch to frame sync if currently on PC
    if (e.detail.field === '__pixels__' && prev !== '__pixels__' &&
        this._storeB && this._syncMode === 'pc') {
      // Trigger sync change to ly=0 (frame alignment)
      this._onSyncChanged({ detail: { sync: 'ly=0' } });
    }
  }

  _onHoverIndex(e) {
    this._hoverIndex = e.detail.index;
  }

  _onCurrentIndex(e) {
    this._currentIndex = e.detail.index;
    // If this came from the table itself (click on row), don't scroll the table
    const fromTable = e.composedPath?.().some(el => el.tagName === 'TRACE-TABLE');
    if (!fromTable) {
      this._scrollTableToCurrent();
    }
  }

  _onScrub(e) {
    this._currentIndex = parseInt(e.target.value, 10);
    this._scrollTableToCurrent();
  }

  _scrollTableToCurrent() {
    if (this._currentIndex == null) return;
    this.updateComplete.then(() => {
      const table = this.renderRoot?.querySelector('trace-table') ||
                    this.renderRoot?.querySelector('trace-diff-table');
      if (table) table.scrollToIndex(this._currentIndex);
    });
  }

  _onHiddenFieldsChanged(e) {
    this._hiddenFields = e.detail.hiddenFields;
  }

  async _onSyncChanged(e) {
    const newSync = e.detail.sync;
    this._syncMode = newSync;

    if (!this._store || !this._storeB) return;

    const bytesA = this._store.originalBytes();
    const bytesB = this._storeB.originalBytes();
    if (!bytesA || !bytesB) {
      console.error('Cannot re-sync: original bytes not available');
      return;
    }

    const { createTraceStore, prepareForDiff } = await import('../lib/wasm-bridge.js');

    try {
      const storeA = await createTraceStore(bytesA);
      const storeB = await createTraceStore(bytesB);

      // Reload ROM if we have a test ROM URL
      if (this._suite && this._testRom) {
        try {
          const { romUrl } = await import('./test-picker.js');
          const resp = await fetch(romUrl(this._suite, this._testRom));
          if (resp.ok) {
            const rom = new Uint8Array(await resp.arrayBuffer());
            storeA.loadRom(rom);
            storeB.loadRom(rom);
          }
        } catch (_) {}
      }

      if (this._store) this._store.free();
      if (this._storeB) this._storeB.free();

      const trigA = storeA.header()?.trigger;
      const trigB = storeB.header()?.trigger;

      const [prepA, prepB] = await prepareForDiff(storeA, storeB, newSync);
      this._store = prepA;
      this._storeB = prepB;
      this._header = prepA.header();
      this._downsampled = (trigA !== trigB);

      this._frameBoundaries = Array.from(this._store.frameBoundaries());
      this._frameBoundariesB = Array.from(this._storeB.frameBoundaries());
      this._viewStart = 0;
      this._viewEnd = this._store.entryCount();

      // Re-apply field exclusions
      const fieldsA = new Set(this._store.header()?.fields || []);
      const fieldsB = new Set(this._storeB.header()?.fields || []);
      const newHidden = new Set(this._hiddenFields);
      if (this._compareHiddenFields) {
        for (const f of this._compareHiddenFields) newHidden.delete(f);
      }
      this._compareHiddenFields = new Set();
      for (const f of fieldsA) {
        if (!fieldsB.has(f)) { newHidden.add(f); this._compareHiddenFields.add(f); }
      }
      for (const f of fieldsB) {
        if (!fieldsA.has(f)) { newHidden.add(f); this._compareHiddenFields.add(f); }
      }
      this._hiddenFields = newHidden;
      this._recomputeDiffStats();
    } catch (err) {
      console.error('Failed to re-sync traces:', err);
    }
  }

  _onViewRangeChanged(e) {
    this._viewStart = e.detail.start;
    this._viewEnd = e.detail.end;
    this._currentIndex = e.detail.start;
    this._recomputeDiffStats();
    this._scrollTableToCurrent();
  }

}

customElements.define('app-shell', AppShell);
