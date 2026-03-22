import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;
const COL_WIDTH = 56;
const IDX_WIDTH = 50;
const ASM_WIDTH = 120;

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
  };

  constructor() {
    super();
    this.store = null;
    this.fields = [];
    this.highlightIndices = null;
    this.hiddenFields = new Set();
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => !this.hiddenFields?.has(f));
  }

  updated(changed) {
    if (changed.has('store') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields')) {
      this._renderedStart = -1;
      this.updateComplete.then(() => this._renderRows());
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
            ${vf.map(f => html`<span style="${hdrStyle(COL_WIDTH)}">${f}</span>`)}
            ${hasRom ? html`<span style="${hdrStyle(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}
          </div>
          <div class="spacer" style="height:${this._spacerHeight()}px"></div>
          <div class="rows"></div>
        </div>
      </div>
    `;
  }

  _spacerHeight() {
    if (!this.store) return 0;
    return Math.min(this.store.entryCount() * ROW_HEIGHT, MAX_SPACER);
  }

  _isRemapped() {
    return this.store && this.store.entryCount() * ROW_HEIGHT > MAX_SPACER;
  }

  _scrollToEntry(scrollTop, scrollEl) {
    if (!this._isRemapped()) return Math.floor(scrollTop / ROW_HEIGHT);
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this.store.entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round((scrollTop / maxScroll) * Math.max(0, maxStart));
  }

  _entryToScroll(index, scrollEl) {
    if (!this._isRemapped()) return index * ROW_HEIGHT;
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const maxStart = this.store.entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
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
    const firstVisible = this._scrollToEntry(scrollEl.scrollTop, scrollEl);
    const containerHeight = scrollEl.clientHeight || 500;
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const startIdx = Math.max(0, firstVisible - OVERSCAN);
    const endIdx = Math.min(this.store.entryCount(), startIdx + visibleCount);
    const count = endIdx - startIdx;

    if (startIdx === this._renderedStart && count === this._renderedCount) return;
    this._renderedStart = startIdx;
    this._renderedCount = count;

    if (count <= 0) { rowsEl.innerHTML = ''; rowsEl.style.top = '0px'; return; }

    let entries;
    try { entries = this.store.entriesRange(startIdx, count); }
    catch (err) { console.error(err); return; }

    if (this._isRemapped()) {
      const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
      const maxStart = this.store.entryCount() - Math.ceil(containerHeight / ROW_HEIGHT);
      rowsEl.style.top = `${Math.round((maxStart > 0 ? startIdx / maxStart : 0) * maxScroll)}px`;
    } else {
      rowsEl.style.top = `${startIdx * ROW_HEIGHT}px`;
    }

    const hasRom = this.store.hasRom?.() ?? false;
    let disasmArr = null;
    if (hasRom) {
      try { disasmArr = this.store.disassembleRange(startIdx, count); } catch (_) {}
    }

    const cs = this._cellStyle.bind(this);
    const hl = this.highlightIndices;
    const parts = [];
    for (let i = 0; i < entries.length; i++) {
      const idx = startIdx + i;
      const data = entries[i];
      const bg = hl?.has(idx) ? 'background:var(--accent-subtle);' : '';
      parts.push(`<div style="display:flex;height:${ROW_HEIGHT}px;align-items:center;border-bottom:1px solid var(--bg);${bg}" data-idx="${idx}">`);
      parts.push(`<span style="${cs(IDX_WIDTH, 'color:var(--text-muted);')}">${idx}</span>`);
      for (const f of vf) {
        parts.push(`<span style="${cs(COL_WIDTH)}">${displayVal(data[f])}</span>`);
      }
      if (disasmArr) {
        parts.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmArr[i] || ''}</span>`);
      }
      parts.push('</div>');
    }
    rowsEl.innerHTML = parts.join('');

    for (const row of rowsEl.children) {
      const idx = parseInt(row.dataset.idx, 10);
      row.addEventListener('mouseenter', () => this._emitHover(idx));
      row.addEventListener('mouseleave', () => this._emitHover(null));
    }
  }

  _emitHover(index) {
    this.dispatchEvent(new CustomEvent('hover-index', {
      detail: { index }, bubbles: true, composed: true,
    }));
  }

  scrollToIndex(index) {
    const scrollEl = this.renderRoot?.querySelector('.container');
    if (!scrollEl) return;
    this._renderedStart = -1;
    scrollEl.scrollTop = this._entryToScroll(index, scrollEl);
    this._renderRows();
  }
}

customElements.define('trace-table', TraceTable);
