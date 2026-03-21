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
      overflow: hidden;
      background: var(--bg-surface);
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    .header-row {
      display: flex;
      background: var(--bg);
      border-bottom: 1px solid var(--border);
      font-size: 0.7rem;
      color: var(--text-muted);
    }
    .header-row span {
      padding: 4px 4px;
      min-width: 36px;
      text-align: right;
      font-family: var(--mono);
    }
    .header-row .idx-col { min-width: 50px; }
    .header-row .sep { width: 2px; min-width: 2px; background: var(--border); padding: 0; }
    .scroll-area {
      flex: 1;
      min-height: 200px;
      overflow-y: auto;
      position: relative;
    }
    .spacer { width: 1px; }
    .rows {
      position: absolute;
      left: 0;
      right: 0;
    }
  `;

  static properties = {
    storeA: { type: Object },
    storeB: { type: Object },
    nameA: { type: String },
    nameB: { type: String },
    fields: { type: Array },
    highlightIndices: { type: Object },
  };

  constructor() {
    super();
    this.storeA = null;
    this.storeB = null;
    this.nameA = 'A';
    this.nameB = 'B';
    this.fields = [];
    this.highlightIndices = null;
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
  }

  updated(changed) {
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('highlightIndices')) {
      this.updateComplete.then(() => this._renderRows());
    }
  }

  render() {
    if (!this.storeA || !this.storeB || !this.fields?.length) return '';
    const displayFields = this.fields.filter(f => f !== 'cy');

    return html`
      <div class="container">
        <div class="header-row">
          <span class="idx-col">#</span>
          ${displayFields.map(f => html`<span title="${this.nameA}: ${f}">${f}</span>`)}
          <span class="sep"></span>
          ${displayFields.map(f => html`<span title="${this.nameB}: ${f}">${f}</span>`)}
        </div>
        <div class="scroll-area" @scroll=${this._onScroll}>
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
    const natural = this._entryCount() * ROW_HEIGHT;
    return Math.min(natural, MAX_SPACER);
  }

  _isRemapped() {
    return this._entryCount() * ROW_HEIGHT > MAX_SPACER;
  }

  _scrollToEntry(scrollTop, scrollEl) {
    if (!this._isRemapped()) return Math.floor(scrollTop / ROW_HEIGHT);
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const frac = scrollTop / maxScroll;
    const maxStart = this._entryCount() - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round(frac * Math.max(0, maxStart));
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
    const scrollEl = this.renderRoot?.querySelector('.scroll-area');
    const rowsEl = this.renderRoot?.querySelector('.rows');
    if (!scrollEl || !rowsEl || !this.storeA || !this.storeB || !this.fields?.length) return;

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

    if (count <= 0) {
      rowsEl.innerHTML = '';
      rowsEl.style.top = '0px';
      return;
    }

    let entriesA, entriesB;
    try {
      entriesA = this.storeA.entriesRange(startIdx, count);
      entriesB = this.storeB.entriesRange(startIdx, count);
    } catch (err) {
      console.error('Failed to fetch entries:', err);
      return;
    }

    if (this._isRemapped()) {
      const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
      const maxStart = total - Math.ceil(containerHeight / ROW_HEIGHT);
      const frac = maxStart > 0 ? startIdx / maxStart : 0;
      rowsEl.style.top = `${Math.round(frac * maxScroll)}px`;
    } else {
      rowsEl.style.top = `${startIdx * ROW_HEIGHT}px`;
    }

    const displayFields = this.fields.filter(f => f !== 'cy');
    const hl = this.highlightIndices;
    const parts = [];

    for (let i = 0; i < entriesA.length; i++) {
      const idx = startIdx + i;
      const a = entriesA[i];
      const b = entriesB[i];

      // Check if any field differs
      let anyDiff = false;
      for (const f of displayFields) {
        if (a[f] !== b[f]) { anyDiff = true; break; }
      }

      const rowCls = hl?.has(idx) ? 'highlight' : '';
      const rowBg = anyDiff ? 'background:rgba(248,81,73,0.06);' : '';

      parts.push(`<div data-idx="${idx}" style="display:flex;height:${ROW_HEIGHT}px;align-items:center;font-family:var(--mono);font-size:0.7rem;border-bottom:1px solid var(--bg);${rowBg}" class="${rowCls}">`);
      parts.push(`<span style="padding:0 4px;min-width:50px;text-align:right;color:var(--text-muted)">${idx}</span>`);

      // Side A values
      for (const f of displayFields) {
        const va = displayVal(a[f]);
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--red)' : '';
        parts.push(`<span style="padding:0 4px;min-width:36px;text-align:right;white-space:nowrap;${color}">${va}</span>`);
      }

      // Separator
      parts.push(`<span style="width:2px;min-width:2px;background:var(--border);align-self:stretch"></span>`);

      // Side B values
      for (const f of displayFields) {
        const vb = displayVal(b[f]);
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--yellow)' : '';
        parts.push(`<span style="padding:0 4px;min-width:36px;text-align:right;white-space:nowrap;${color}">${vb}</span>`);
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
      detail: { index },
      bubbles: true, composed: true,
    }));
  }

  scrollToIndex(index) {
    const scrollEl = this.renderRoot?.querySelector('.scroll-area');
    if (!scrollEl) return;
    this._renderedStart = -1;
    scrollEl.scrollTop = this._entryToScroll(index, scrollEl);
    this._renderRows();
  }
}

customElements.define('trace-diff-table', TraceDiffTable);
