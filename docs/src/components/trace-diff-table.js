import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;
const COL_WIDTH = 48;
const IDX_WIDTH = 50;
const ASM_WIDTH = 100;

export class TraceDiffTable extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .split {
      display: flex;
      flex: 1;
      min-height: 200px;
      gap: 0;
      border: 1px solid var(--border);
      border-radius: 8px;
      overflow: hidden;
    }
    .panel {
      flex: 1;
      overflow: auto;
      background: var(--bg-surface);
      position: relative;
    }
    .panel:first-child {
      border-right: 2px solid var(--accent);
    }
    .panel:last-child {
      border-left: 2px solid #d29922;
    }
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
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
    this._syncing = false;
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => f !== 'cy' && !this.hiddenFields?.has(f));
  }

  updated(changed) {
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields')) {
      this._renderedStart = -1;
      this.updateComplete.then(() => {
        this._setupSyncScroll();
        this._renderRows();
      });
    }
  }

  _cellStyle(width, extra = '') {
    return `padding:0 4px;width:${width}px;min-width:${width}px;max-width:${width}px;text-align:right;white-space:nowrap;font-family:var(--mono);font-size:0.7rem;box-sizing:border-box;${extra}`;
  }

  render() {
    if (!this.storeA || !this.storeB || !this.fields?.length) return '';
    const vf = this._visibleFields;
    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;

    const hdrStyle = (w, extra = '') => `${this._cellStyle(w, extra)}padding-top:6px;padding-bottom:6px;color:var(--text-muted);`;

    const panelHeader = (name, color) => html`
      <div class="header-row">
        <span style="${hdrStyle(IDX_WIDTH)}"><span style="color:${color};font-weight:600">${name}</span> #</span>
        ${hasRom ? html`<span style="${hdrStyle(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}
        ${vf.map(f => html`<span style="${hdrStyle(COL_WIDTH)}">${f}</span>`)}
      </div>
    `;

    return html`
      <div class="split">
        <div class="panel" id="panel-a" @scroll=${this._onScroll}>
          <div class="inner">
            ${panelHeader(this.nameA, '#58a6ff')}
            <div class="spacer" style="height:${this._spacerHeight()}px"></div>
            <div class="rows" id="rows-a"></div>
          </div>
        </div>
        <div class="panel" id="panel-b" @scroll=${this._onScrollB}>
          <div class="inner">
            ${panelHeader(this.nameB, '#d29922')}
            <div class="spacer" style="height:${this._spacerHeight()}px"></div>
            <div class="rows" id="rows-b"></div>
          </div>
        </div>
      </div>
    `;
  }

  _setupSyncScroll() {
    // Sync is handled by _onScroll and _onScrollB
  }

  _onScroll(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelB = this.renderRoot?.querySelector('#panel-b');
    const panelA = e.target;
    if (panelB) {
      panelB.scrollTop = panelA.scrollTop;
      panelB.scrollLeft = panelA.scrollLeft;
    }
    this._syncing = false;
    this._scheduleRender();
  }

  _onScrollB(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const panelB = e.target;
    if (panelA) {
      panelA.scrollTop = panelB.scrollTop;
      panelA.scrollLeft = panelB.scrollLeft;
    }
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
    return Math.min(this.storeA.entryCount(), this.storeB.entryCount());
  }

  _spacerHeight() {
    return Math.min(this._entryCount() * ROW_HEIGHT, MAX_SPACER);
  }

  _isRemapped() {
    return this._entryCount() * ROW_HEIGHT > MAX_SPACER;
  }

  _scrollToEntry(scrollTop, scrollEl) {
    if (!this._isRemapped()) return Math.floor(scrollTop / ROW_HEIGHT);
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this._entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round((scrollTop / maxScroll) * Math.max(0, maxStart));
  }

  _entryToScroll(index, scrollEl) {
    if (!this._isRemapped()) return index * ROW_HEIGHT;
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
    if (!panelA || !rowsA || !rowsB || !this.storeA || !this.storeB) return;

    const vf = this._visibleFields;
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
      rowsA.innerHTML = ''; rowsB.innerHTML = '';
      rowsA.style.top = '0px'; rowsB.style.top = '0px';
      return;
    }

    let entriesA, entriesB;
    try {
      entriesA = this.storeA.entriesRange(startIdx, count);
      entriesB = this.storeB.entriesRange(startIdx, count);
    } catch (err) { console.error(err); return; }

    let top;
    if (this._isRemapped()) {
      const maxScroll = panelA.scrollHeight - panelA.clientHeight;
      const maxStart = total - Math.ceil(containerHeight / ROW_HEIGHT);
      top = Math.round((maxStart > 0 ? startIdx / maxStart : 0) * maxScroll);
    } else {
      top = startIdx * ROW_HEIGHT;
    }
    rowsA.style.top = `${top}px`;
    rowsB.style.top = `${top}px`;

    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;
    let disasmArr = null;
    if (hasRom) {
      const ds = this.storeA.hasRom?.() ? this.storeA : this.storeB;
      try { disasmArr = ds.disassembleRange(startIdx, count); } catch (_) {}
    }

    const cs = this._cellStyle.bind(this);
    const hl = this.highlightIndices;
    const partsA = [];
    const partsB = [];

    for (let i = 0; i < entriesA.length; i++) {
      const idx = startIdx + i;
      const a = entriesA[i];
      const b = entriesB[i];

      let anyDiff = false;
      for (const f of vf) {
        if (a[f] !== b[f]) { anyDiff = true; break; }
      }

      const hlBg = hl?.has(idx) ? 'background:var(--accent-subtle);' : '';
      const diffBg = anyDiff ? 'background:rgba(248,81,73,0.06);' : '';
      const bg = hlBg || diffBg;
      const rowStart = `<div data-idx="${idx}" style="display:flex;height:${ROW_HEIGHT}px;align-items:center;border-bottom:1px solid var(--bg);${bg}">`;

      // Panel A
      partsA.push(rowStart);
      partsA.push(`<span style="${cs(IDX_WIDTH, 'color:var(--text-muted);')}">${idx}</span>`);
      if (disasmArr) {
        partsA.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmArr[i] || ''}</span>`);
      }
      for (const f of vf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--red);font-weight:600;' : '';
        partsA.push(`<span style="${cs(COL_WIDTH, color)}">${displayVal(a[f])}</span>`);
      }
      partsA.push('</div>');

      // Panel B
      partsB.push(rowStart);
      partsB.push(`<span style="${cs(IDX_WIDTH, 'color:var(--text-muted);')}">${idx}</span>`);
      if (disasmArr) {
        partsB.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmArr[i] || ''}</span>`);
      }
      for (const f of vf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--yellow);font-weight:600;' : '';
        partsB.push(`<span style="${cs(COL_WIDTH, color)}">${displayVal(b[f])}</span>`);
      }
      partsB.push('</div>');
    }

    rowsA.innerHTML = partsA.join('');
    rowsB.innerHTML = partsB.join('');

    // Hover events on both panels
    for (const rows of [rowsA, rowsB]) {
      for (const row of rows.children) {
        const idx = parseInt(row.dataset.idx, 10);
        row.addEventListener('mouseenter', () => this._emitHover(idx));
        row.addEventListener('mouseleave', () => this._emitHover(null));
      }
    }
  }

  _emitHover(index) {
    this.dispatchEvent(new CustomEvent('hover-index', {
      detail: { index }, bubbles: true, composed: true,
    }));
  }

  scrollToIndex(index) {
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const panelB = this.renderRoot?.querySelector('#panel-b');
    if (!panelA) return;
    this._renderedStart = -1;
    const scrollTop = this._entryToScroll(index, panelA);
    panelA.scrollTop = scrollTop;
    if (panelB) panelB.scrollTop = scrollTop;
    this._renderRows();
  }
}

customElements.define('trace-diff-table', TraceDiffTable);
