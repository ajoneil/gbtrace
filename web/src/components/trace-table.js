import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const HEADER_HEIGHT = 28;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;
const COL_WIDTH = 56;
const IDX_WIDTH = 50;
const ASM_WIDTH = 120;
const PIX_WIDTH = 28;
const PIX_COLORS = ['#e0f8d0', '#88c070', '#346856', '#081820'];

export class TraceTable extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .container {
      border: 1px solid var(--border);
      border-radius: 8px;
      overflow: auto;
      background: var(--bg-surface);
      flex: 1;
      min-height: 200px;
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
    store: { type: Object },
    fields: { type: Array },
    highlightIndices: { type: Object },
    hiddenFields: { type: Object },
    viewStart: { type: Number },
    viewEnd: { type: Number },
    tcyclePixels: { type: Boolean },
    currentIndex: { type: Number },
  };

  constructor() {
    super();
    this.store = null;
    this.fields = [];
    this.highlightIndices = null;
    this.hiddenFields = new Set();
    this.viewStart = 0;
    this.viewEnd = 0;
    this.tcyclePixels = false;
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => !this.hiddenFields?.has(f));
  }

  updated(changed) {
    if (changed.has('store') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields') || changed.has('viewStart') || changed.has('viewEnd')) {
      this._renderedStart = -1;
      this.updateComplete.then(() => this._renderRows());
    } else if (changed.has('currentIndex')) {
      // Force re-render of visible rows to update highlight
      this._renderedStart = -1;
      this._renderRows();
    }
  }

  _cellStyle(width, extra = '') {
    return `padding:0 4px;width:${width}px;min-width:${width}px;max-width:${width}px;text-align:right;white-space:nowrap;font-family:var(--mono);font-size:0.75rem;box-sizing:border-box;${extra}`;
  }

  render() {
    if (!this.store || !this.fields?.length) return '';
    const vf = this._visibleFields;
    const hasRom = this.store.hasRom?.() ?? false;

    const hdrStyle = (w, extra = '') => `${this._cellStyle(w, extra)}padding-top:6px;padding-bottom:6px;color:var(--text-muted);`;

    return html`
      <div class="container" @scroll=${this._onScroll}>
        <div class="inner">
          <div class="header-row">
            <span style="${hdrStyle(IDX_WIDTH)}">#</span>
            ${this.tcyclePixels ? html`<span style="${hdrStyle(PIX_WIDTH)}">pix</span>` : ''}
            ${vf.map(f => html`
              <span style="${hdrStyle(COL_WIDTH)}">${f}</span>
              ${hasRom && f === 'pc' ? html`<span style="${hdrStyle(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}
            `)}
          </div>
          <div class="spacer" style="height:${this._spacerHeight()}px"></div>
          <div class="rows"></div>
        </div>
      </div>
    `;
  }

  /** Number of entries in the current view range. */
  get _viewCount() {
    if (!this.store) return 0;
    const end = this.viewEnd || this.store.entryCount();
    return end - (this.viewStart || 0);
  }

  _spacerHeight() {
    if (!this.store) return 0;
    return Math.min(this._viewCount * ROW_HEIGHT + HEADER_HEIGHT, MAX_SPACER);
  }

  _isRemapped() {
    return this.store && this._viewCount * ROW_HEIGHT > MAX_SPACER;
  }

  _scrollToEntry(scrollTop, scrollEl) {
    const adjusted = Math.max(0, scrollTop - HEADER_HEIGHT);
    if (!this._isRemapped()) return Math.floor(adjusted / ROW_HEIGHT);
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this._viewCount - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round((scrollTop / maxScroll) * Math.max(0, maxStart));
  }

  _entryToScroll(index, scrollEl) {
    // index here is relative to the view (0 = first entry in view)
    if (!this._isRemapped()) return index * ROW_HEIGHT + HEADER_HEIGHT;
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this._viewCount - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    if (maxStart <= 0) return 0;
    return Math.round((index / maxStart) * maxScroll);
  }

  _onScroll() {
    if (this._rafId) return;
    this._rafId = requestAnimationFrame(() => {
      this._rafId = null;
      this._renderRows();
    });
  }

  _renderRows() {
    const scrollEl = this.renderRoot?.querySelector('.container');
    const rowsEl = this.renderRoot?.querySelector('.rows');
    if (!scrollEl || !rowsEl || !this.store || !this.fields?.length) return;

    const vf = this._visibleFields;
    const vs = this.viewStart || 0;
    const ve = this.viewEnd || this.store.entryCount();
    const firstVisible = this._scrollToEntry(scrollEl.scrollTop, scrollEl);
    const containerHeight = scrollEl.clientHeight || 500;
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const startIdx = Math.max(0, firstVisible - OVERSCAN);
    const endIdx = Math.min(ve - vs, startIdx + visibleCount);
    const count = endIdx - startIdx;
    // Global index = vs + startIdx
    const globalStart = vs + startIdx;

    if (startIdx === this._renderedStart && count === this._renderedCount) return;
    this._renderedStart = startIdx;
    this._renderedCount = count;

    if (count <= 0) { rowsEl.innerHTML = ''; rowsEl.style.top = `${HEADER_HEIGHT}px`; return; }

    let entries;
    try { entries = this.store.entriesRange(globalStart, count); }
    catch (err) { console.error(err); return; }

    if (this._isRemapped()) {
      const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
      const maxStart = this._viewCount - Math.ceil(containerHeight / ROW_HEIGHT);
      rowsEl.style.top = `${Math.round((maxStart > 0 ? startIdx / maxStart : 0) * maxScroll) + HEADER_HEIGHT}px`;
    } else {
      rowsEl.style.top = `${startIdx * ROW_HEIGHT + HEADER_HEIGHT}px`;
    }

    const hasRom = this.store.hasRom?.() ?? false;
    let disasmArr = null;
    if (hasRom) {
      try { disasmArr = this.store.disassembleRange(globalStart, count); } catch (_) {}
    }

    // Fetch pixel values for T-cycle traces
    let pixArr = null;
    if (this.tcyclePixels) {
      try { pixArr = this.store.pixRange(globalStart, count); } catch (_) {}
    }

    const cs = this._cellStyle.bind(this);
    const hl = this.highlightIndices;
    const parts = [];
    for (let i = 0; i < entries.length; i++) {
      const globalIdx = globalStart + i;
      const data = entries[i];
      const isCurrent = this.currentIndex != null && globalIdx === this.currentIndex;
      const bg = isCurrent
        ? 'background:rgba(88,166,255,0.09);border-left:3px solid var(--accent);'
        : hl?.has(globalIdx) ? 'background:var(--accent-subtle);' : 'border-left:3px solid transparent;';
      parts.push(`<div style="display:flex;height:${ROW_HEIGHT}px;align-items:center;border-bottom:1px solid var(--bg);${bg}" data-idx="${globalIdx}">`);
      parts.push(`<span style="${cs(IDX_WIDTH, 'color:var(--text-muted);')}">${globalIdx}</span>`);
      if (pixArr) {
        const pv = pixArr[i];
        if (pv <= 3) {
          parts.push(`<span style="${cs(PIX_WIDTH, 'text-align:center;')}"><span style="display:inline-block;width:10px;height:10px;border-radius:2px;background:${PIX_COLORS[pv]};border:1px solid var(--border);"></span></span>`);
        } else {
          parts.push(`<span style="${cs(PIX_WIDTH)}"></span>`);
        }
      }
      for (const f of vf) {
        parts.push(`<span style="${cs(COL_WIDTH)}">${displayVal(data[f], f)}</span>`);
        if (disasmArr && f === 'pc') {
          parts.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmArr[i] || ''}</span>`);
        }
      }
      parts.push('</div>');
    }
    rowsEl.innerHTML = parts.join('');

    for (const row of rowsEl.children) {
      const idx = parseInt(row.dataset.idx, 10);
      row.addEventListener('mouseenter', () => this._emitHover(idx));
      row.addEventListener('mouseleave', () => this._emitHover(null));
      row.addEventListener('click', () => this._emitCurrent(idx));
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
    const scrollEl = this.renderRoot?.querySelector('.container');
    if (!scrollEl) return;
    this._renderedStart = -1;
    // Convert global index to view-relative, offset by a few rows
    // so the target isn't hidden under the sticky header
    const vs = this.viewStart || 0;
    const relIndex = Math.max(0, index - vs);
    const offsetRows = 3;
    const targetIndex = Math.max(0, relIndex - offsetRows);
    scrollEl.scrollTop = this._entryToScroll(targetIndex, scrollEl);
    this._renderRows();
  }
}

customElements.define('trace-table', TraceTable);
