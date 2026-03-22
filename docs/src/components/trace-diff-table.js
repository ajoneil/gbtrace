import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;

export class TraceDiffTable extends LitElement {
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
      position: relative;
    }
    .inner {
      min-width: fit-content;
      position: relative;
    }
    .header-row {
      display: flex;
      background: var(--bg);
      border-bottom: 1px solid var(--border);
      font-size: 0.7rem;
      color: var(--text-muted);
      position: sticky;
      top: 0;
      z-index: 2;
    }
    .header-row span {
      padding: 4px 4px;
      min-width: 36px;
      text-align: right;
      font-family: var(--mono);
      white-space: nowrap;
    }
    .header-row .idx-col { min-width: 50px; }
    .header-row .asm-col { min-width: 100px; text-align: left; }
    .header-row .sep { width: 2px; min-width: 2px; background: var(--border); padding: 0; }
    .header-row .side-a { color: #58a6ff; }
    .header-row .side-b { color: #d29922; }
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
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => f !== 'cy' && !this.hiddenFields?.has(f));
  }

  updated(changed) {
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields')) {
      this._renderedStart = -1;
      this.updateComplete.then(() => this._renderRows());
    }
  }

  render() {
    if (!this.storeA || !this.storeB || !this.fields?.length) return '';
    const vf = this._visibleFields;
    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;

    return html`
      <div class="container" @scroll=${this._onScroll}>
        <div class="inner">
          <div class="header-row">
            <span class="idx-col">#</span>
            ${hasRom ? html`<span class="asm-col">asm</span>` : ''}
            ${vf.map(f => html`<span class="side-a" title="${this.nameA}: ${f}">${f}</span>`)}
            <span class="sep"></span>
            ${vf.map(f => html`<span class="side-b" title="${this.nameB}: ${f}">${f}</span>`)}
          </div>
          <div class="spacer" style="height:${this._spacerHeight()}px"></div>
          <div class="rows"></div>
        </div>
      </div>
    `;
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
    if (!scrollEl || !rowsEl || !this.storeA || !this.storeB || !this.fields?.length) return;

    const vf = this._visibleFields;
    const total = this._entryCount();
    const firstVisible = this._scrollToEntry(scrollEl.scrollTop, scrollEl);
    const containerHeight = scrollEl.clientHeight || 500;
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const startIdx = Math.max(0, firstVisible - OVERSCAN);
    const endIdx = Math.min(total, startIdx + visibleCount);
    const count = endIdx - startIdx;

    if (startIdx === this._renderedStart && count === this._renderedCount) return;
    this._renderedStart = startIdx;
    this._renderedCount = count;

    if (count <= 0) { rowsEl.innerHTML = ''; rowsEl.style.top = '0px'; return; }

    let entriesA, entriesB;
    try {
      entriesA = this.storeA.entriesRange(startIdx, count);
      entriesB = this.storeB.entriesRange(startIdx, count);
    } catch (err) { console.error(err); return; }

    if (this._isRemapped()) {
      const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
      const maxStart = total - Math.ceil(containerHeight / ROW_HEIGHT);
      rowsEl.style.top = `${Math.round((maxStart > 0 ? startIdx / maxStart : 0) * maxScroll)}px`;
    } else {
      rowsEl.style.top = `${startIdx * ROW_HEIGHT}px`;
    }

    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;
    let disasmArr = null;
    if (hasRom) {
      const ds = this.storeA.hasRom?.() ? this.storeA : this.storeB;
      try { disasmArr = ds.disassembleRange(startIdx, count); } catch (_) {}
    }

    const hl = this.highlightIndices;
    const parts = [];

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

      parts.push(`<div data-idx="${idx}" style="display:flex;height:${ROW_HEIGHT}px;align-items:center;font-family:var(--mono);font-size:0.7rem;border-bottom:1px solid var(--bg);${bg}">`);
      parts.push(`<span style="padding:0 4px;min-width:50px;text-align:right;color:var(--text-muted)">${idx}</span>`);

      if (disasmArr) {
        parts.push(`<span style="padding:0 4px;min-width:100px;text-align:left;color:var(--green);white-space:nowrap">${disasmArr[i] || ''}</span>`);
      }

      for (const f of vf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--red)' : '';
        parts.push(`<span style="padding:0 4px;min-width:36px;text-align:right;white-space:nowrap;${color}">${displayVal(a[f])}</span>`);
      }

      parts.push(`<span style="width:2px;min-width:2px;background:var(--border);align-self:stretch"></span>`);

      for (const f of vf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--yellow)' : '';
        parts.push(`<span style="padding:0 4px;min-width:36px;text-align:right;white-space:nowrap;${color}">${displayVal(b[f])}</span>`);
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

customElements.define('trace-diff-table', TraceDiffTable);
