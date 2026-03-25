import { LitElement, html, css } from 'lit';
import { displayVal, displayFlagsDiff } from '../lib/format.js';

const ROW_HEIGHT = 24;
const HEADER_HEIGHT = 28;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;
const COL_WIDTH = 48;
const IDX_WIDTH = 50;
const PC_WIDTH = 48;
const ASM_WIDTH = 100;

export class TraceDiffTable extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .outer {
      display: flex;
      flex: 1;
      min-height: 200px;
      border: 1px solid var(--border);
      border-radius: 8px;
      overflow: hidden;
    }
    .shared {
      flex-shrink: 0;
      overflow: hidden;
      background: var(--bg-surface);
      border-right: 1px solid var(--border);
      position: relative;
    }
    .shared .inner { position: relative; }
    .panels {
      display: flex;
      flex: 1;
      min-width: 0;
    }
    .panel {
      flex: 1;
      overflow: auto;
      background: var(--bg-surface);
      position: relative;
    }
    .panel-a { border-right: 2px solid #58a6ff; }
    .panel-b { border-left: 2px solid #d29922; }
    .inner {
      min-width: fit-content;
      position: relative;
    }
    .header-row {
      display: flex;
      background: var(--bg);
      border-bottom: 1px solid var(--border);
      position: sticky;
      top: 0;
      z-index: 2;
    }
    .spacer { width: 1px; }
    .rows { position: absolute; left: 0; right: 0; }
  `;

  static properties = {
    storeA: { type: Object },
    storeB: { type: Object },
    nameA: { type: String },
    nameB: { type: String },
    fields: { type: Array },
    highlightIndices: { type: Object },
    hiddenFields: { type: Object },
    viewStart: { type: Number },
    viewEnd: { type: Number },
    currentIndex: { type: Number },
    _pcMatches: { state: true },
  };

  constructor() {
    super();
    this.storeA = null;
    this.storeB = null;
    this.nameA = 'A';
    this.nameB = 'B';
    this.fields = [];
    this.highlightIndices = null;
    this.hiddenFields = new Set();
    this.viewStart = 0;
    this.viewEnd = 0;
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
    this._syncing = false;
    this._pcMatches = true;
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => !this.hiddenFields?.has(f));
  }

  /** Fields shown in the per-side panels (exclude shared fields). */
  get _sideFields() {
    const shared = this._sharedFields;
    return this._visibleFields.filter(f => !shared.has(f));
  }

  /** Fields pulled into the shared left column. */
  get _sharedFields() {
    const s = new Set();
    if (this._pcMatches) {
      if (this._visibleFields.includes('pc')) s.add('pc');
    }
    return s;
  }

  updated(changed) {
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('hiddenFields') || changed.has('viewStart') || changed.has('viewEnd')) {
      this._renderedStart = -1;
      this._checkPcMatch();
    }
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields') || changed.has('_pcMatches') || changed.has('viewStart') || changed.has('viewEnd') || changed.has('currentIndex')) {
      // Force re-render when currentIndex changes (bypass start/count cache check)
      if (changed.has('currentIndex')) this._renderedStart = -1;
      this.updateComplete.then(() => {
        this._renderRows();
      });
    }
  }

  /** Check if PC values match between traces. Only share the PC column if
   *  they're identical (or nearly so) — any early divergence means they should
   *  be shown separately for comparison. */
  _checkPcMatch() {
    if (!this.storeA || !this.storeB) { this._pcMatches = false; return; }
    try {
      const indices = this.storeA.diffField(this.storeB, 'pc');
      this._pcMatches = indices.length === 0;
    } catch (_) {
      this._pcMatches = false;
    }
  }

  _cs(width, extra = '') {
    return `padding:0 4px;width:${width}px;min-width:${width}px;max-width:${width}px;text-align:right;white-space:nowrap;font-family:var(--mono);font-size:0.7rem;box-sizing:border-box;${extra}`;
  }

  _hdr(width, extra = '') {
    return `${this._cs(width, extra)}padding-top:6px;padding-bottom:6px;color:var(--text-muted);`;
  }

  /**
   * Column definitions for each panel. Both header and row rendering
   * use these to stay in sync.
   * Returns { shared: [...], side: [...] } where each entry is
   * { name, width, align, type } with type being 'field' or 'asm'.
   */
  _getColumns() {
    const shared = this._sharedFields;
    const sf = this._sideFields;
    const hasRom = (this.storeA?.hasRom?.() || this.storeB?.hasRom?.()) ?? false;

    const sharedCols = [{ name: '#', width: IDX_WIDTH, align: 'right', type: 'idx' }];
    if (shared.has('pc')) {
      sharedCols.push({ name: 'pc', width: PC_WIDTH, align: 'right', type: 'field' });
      if (hasRom) sharedCols.push({ name: 'asm', width: ASM_WIDTH, align: 'left', type: 'asm' });
    }

    const sideCols = [];
    for (const f of sf) {
      sideCols.push({ name: f, width: COL_WIDTH, align: 'right', type: 'field' });
      if (f === 'pc' && hasRom && !shared.has('pc')) {
        sideCols.push({ name: 'asm', width: ASM_WIDTH, align: 'left', type: 'asm' });
      }
    }

    return { shared: sharedCols, side: sideCols };
  }

  _renderHeader(cols) {
    return cols.map(c =>
      html`<span style="${this._hdr(c.width, c.align === 'left' ? 'text-align:left;' : '')}">${c.name}</span>`
    );
  }

  render() {
    if (!this.storeA || !this.storeB || !this.fields?.length) return '';
    const { shared: sharedCols, side: sideCols } = this._getColumns();

    return html`
      <div class="outer">
        <div class="shared" id="shared-panel">
          <div class="inner">
            <div class="header-row">${this._renderHeader(sharedCols)}</div>
            <div class="spacer" style="height:${this._spacerHeight()}px"></div>
            <div class="rows" id="rows-shared"></div>
          </div>
        </div>
        <div class="panels">
          <div class="panel panel-a" id="panel-a" @scroll=${this._onScrollA}>
            <div class="inner">
              <div class="header-row">${this._renderHeader(sideCols)}</div>
              <div class="spacer" style="height:${this._spacerHeight()}px"></div>
              <div class="rows" id="rows-a"></div>
            </div>
          </div>
          <div class="panel panel-b" id="panel-b" @scroll=${this._onScrollB}>
            <div class="inner">
              <div class="header-row">${this._renderHeader(sideCols)}</div>
              <div class="spacer" style="height:${this._spacerHeight()}px"></div>
              <div class="rows" id="rows-b"></div>
            </div>
          </div>
        </div>
      </div>
    `;
  }

  // Hmm, the header for A/B panels is wrong - the first field name is replaced by the emu name.
  // Let me fix the render to show all side fields with a proper header.

  _onScrollA(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelB = this.renderRoot?.querySelector('#panel-b');
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (panelB) {
      panelB.scrollTop = e.target.scrollTop;
      panelB.scrollLeft = e.target.scrollLeft;
    }
    if (shared) shared.scrollTop = e.target.scrollTop;
    this._syncing = false;
    this._scheduleRender();
  }

  _onScrollB(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (panelA) {
      panelA.scrollTop = e.target.scrollTop;
      panelA.scrollLeft = e.target.scrollLeft;
    }
    if (shared) shared.scrollTop = e.target.scrollTop;
    this._syncing = false;
    this._scheduleRender();
  }

  _scheduleRender() {
    if (this._rafId) return;
    this._rafId = requestAnimationFrame(() => {
      this._rafId = null;
      this._renderRows();
    });
  }

  _entryCount() {
    const ve = this.viewEnd || Math.min(this.storeA.entryCount(), this.storeB.entryCount());
    const vs = this.viewStart || 0;
    return ve - vs;
  }

  _spacerHeight() {
    return Math.min(this._entryCount() * ROW_HEIGHT + HEADER_HEIGHT, MAX_SPACER);
  }

  _isRemapped() {
    return this._entryCount() * ROW_HEIGHT > MAX_SPACER;
  }

  _scrollToEntry(scrollTop, scrollEl) {
    const adjusted = Math.max(0, scrollTop - HEADER_HEIGHT);
    if (!this._isRemapped()) return Math.floor(adjusted / ROW_HEIGHT);
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this._entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round((scrollTop / maxScroll) * Math.max(0, maxStart));
  }

  _entryToScroll(index, scrollEl) {
    if (!this._isRemapped()) return index * ROW_HEIGHT + HEADER_HEIGHT;
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this._entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    if (maxStart <= 0) return 0;
    return Math.round((index / maxStart) * maxScroll);
  }

  _renderRows() {
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const rowsA = this.renderRoot?.querySelector('#rows-a');
    const rowsB = this.renderRoot?.querySelector('#rows-b');
    const rowsShared = this.renderRoot?.querySelector('#rows-shared');
    if (!panelA || !rowsA || !rowsB || !rowsShared || !this.storeA || !this.storeB) return;

    const total = this._entryCount();
    const firstVisible = this._scrollToEntry(panelA.scrollTop, panelA);
    const containerHeight = panelA.clientHeight || 500;
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const startIdx = Math.max(0, firstVisible - OVERSCAN);
    const endIdx = Math.min(total, startIdx + visibleCount);
    const count = endIdx - startIdx;

    if (startIdx === this._renderedStart && count === this._renderedCount) return;
    this._renderedStart = startIdx;
    this._renderedCount = count;

    if (count <= 0) {
      rowsA.innerHTML = ''; rowsB.innerHTML = ''; rowsShared.innerHTML = '';
      rowsA.style.top = rowsB.style.top = rowsShared.style.top = `${HEADER_HEIGHT}px`;
      return;
    }

    const vs = this.viewStart || 0;
    const globalStart = vs + startIdx;
    let entriesA, entriesB;
    try {
      entriesA = this.storeA.entriesRange(globalStart, count);
      entriesB = this.storeB.entriesRange(globalStart, count);
    } catch (err) { console.error(err); return; }

    let top;
    if (this._isRemapped()) {
      const maxScroll = panelA.scrollHeight - panelA.clientHeight;
      const maxStart = total - Math.ceil(containerHeight / ROW_HEIGHT);
      top = Math.round((maxStart > 0 ? startIdx / maxStart : 0) * maxScroll) + HEADER_HEIGHT;
    } else {
      top = startIdx * ROW_HEIGHT + HEADER_HEIGHT;
    }
    rowsA.style.top = `${top}px`;
    rowsB.style.top = `${top}px`;
    rowsShared.style.top = `${top}px`;

    // Use the same column definitions as the header
    const { shared: sharedCols, side: sideCols } = this._getColumns();

    // Pre-fetch disassembly arrays
    let disasmA = null, disasmB = null;
    const hasSharedAsm = sharedCols.some(c => c.type === 'asm');
    const hasSideAsm = sideCols.some(c => c.type === 'asm');
    if (hasSharedAsm) {
      const ds = this.storeA.hasRom?.() ? this.storeA : this.storeB;
      try { disasmA = ds.disassembleRange(startIdx, count); } catch (_) {}
    }
    if (hasSideAsm) {
      if (this.storeA.hasRom?.()) try { disasmA = this.storeA.disassembleRange(startIdx, count); } catch (_) {}
      if (this.storeB.hasRom?.()) try { disasmB = this.storeB.disassembleRange(startIdx, count); } catch (_) {}
    }

    const cs = this._cs.bind(this);
    const hl = this.highlightIndices;
    const partsShared = [];
    const partsA = [];
    const partsB = [];

    const len = Math.min(entriesA.length, entriesB.length);
    for (let i = 0; i < len; i++) {
      const idx = globalStart + i;
      const a = entriesA[i];
      const b = entriesB[i];
      if (!a || !b) continue;

      // Check for any visible field difference
      let anyDiff = false;
      for (const c of [...sharedCols, ...sideCols]) {
        if (c.type === 'field' && a[c.name] !== b[c.name]) { anyDiff = true; break; }
      }

      const isCurrent = this.currentIndex != null && idx === this.currentIndex;
      const currentBg = isCurrent ? 'background:rgba(88,166,255,0.09);border-left:3px solid var(--accent);' : 'border-left:3px solid transparent;';
      const hlBg = !isCurrent && hl?.has(idx) ? 'background:var(--accent-subtle);' : '';
      const diffBg = !isCurrent && !hlBg && anyDiff ? 'background:rgba(248,81,73,0.06);' : '';
      const bg = currentBg + (hlBg || diffBg);
      const rowStart = `<div data-idx="${idx}" style="display:flex;height:${ROW_HEIGHT}px;align-items:center;border-bottom:1px solid var(--bg);${bg}">`;

      // Shared panel — iterate sharedCols
      partsShared.push(rowStart);
      for (const c of sharedCols) {
        if (c.type === 'idx') {
          partsShared.push(`<span style="${cs(c.width, 'color:var(--text-muted);')}">${idx}</span>`);
        } else if (c.type === 'field') {
          const differs = a[c.name] !== b[c.name];
          partsShared.push(`<span style="${cs(c.width, differs ? 'color:var(--red);' : '')}">${displayVal(a[c.name], c.name)}</span>`);
        } else if (c.type === 'asm') {
          partsShared.push(`<span style="${cs(c.width, 'text-align:left;color:var(--green);')}">${disasmA?.[i] || ''}</span>`);
        }
      }
      partsShared.push('</div>');

      // Panel A — iterate sideCols
      partsA.push(rowStart);
      for (const c of sideCols) {
        if (c.type === 'field') {
          const differs = a[c.name] !== b[c.name];
          if (c.name === 'f' && differs) {
            partsA.push(`<span style="${cs(c.width)}">${displayFlagsDiff(a.f, b.f, 'var(--red)')}</span>`);
          } else {
            partsA.push(`<span style="${cs(c.width, differs ? 'color:var(--red);font-weight:600;' : '')}">${displayVal(a[c.name], c.name)}</span>`);
          }
        } else if (c.type === 'asm') {
          partsA.push(`<span style="${cs(c.width, 'text-align:left;color:var(--green);')}">${disasmA?.[i] || ''}</span>`);
        }
      }
      partsA.push('</div>');

      // Panel B — iterate sideCols
      partsB.push(rowStart);
      for (const c of sideCols) {
        if (c.type === 'field') {
          const differs = a[c.name] !== b[c.name];
          if (c.name === 'f' && differs) {
            partsB.push(`<span style="${cs(c.width)}">${displayFlagsDiff(b.f, a.f, 'var(--yellow)')}</span>`);
          } else {
            partsB.push(`<span style="${cs(c.width, differs ? 'color:var(--yellow);font-weight:600;' : '')}">${displayVal(b[c.name], c.name)}</span>`);
          }
        } else if (c.type === 'asm') {
          partsB.push(`<span style="${cs(c.width, 'text-align:left;color:var(--green);')}">${disasmB?.[i] || disasmA?.[i] || ''}</span>`);
        }
      }
      partsB.push('</div>');
    }

    rowsShared.innerHTML = partsShared.join('');
    rowsA.innerHTML = partsA.join('');
    rowsB.innerHTML = partsB.join('');

    for (const rows of [rowsShared, rowsA, rowsB]) {
      for (const row of rows.children) {
        const idx = parseInt(row.dataset.idx, 10);
        row.addEventListener('mouseenter', () => this._emitHover(idx));
        row.addEventListener('mouseleave', () => this._emitHover(null));
        row.addEventListener('click', () => this._emitCurrent(idx));
      }
    }
  }

  _emitHover(index) {
    this.dispatchEvent(new CustomEvent('hover-index', {
      detail: { index }, bubbles: true, composed: true,
    }));
  }

  _emitCurrent(index) {
    this.dispatchEvent(new CustomEvent('current-index', {
      detail: { index }, bubbles: true, composed: true,
    }));
  }

  scrollToIndex(index) {
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const panelB = this.renderRoot?.querySelector('#panel-b');
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (!panelA) return;
    this._renderedStart = -1;
    const scrollTop = this._entryToScroll(index, panelA);
    panelA.scrollTop = scrollTop;
    if (panelB) panelB.scrollTop = scrollTop;
    if (shared) shared.scrollTop = scrollTop;
    this._renderRows();
  }
}

customElements.define('trace-diff-table', TraceDiffTable);
