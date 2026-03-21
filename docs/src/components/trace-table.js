import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;
// Max spacer height — browsers clamp element sizes around 16M-33M px.
// Use a safe value well under the limit.
const MAX_SPACER = 10_000_000;

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
      font-size: 0.75rem;
      color: var(--text-muted);
      position: sticky;
      top: 0;
      z-index: 1;
    }
    .header-row span {
      padding: 6px 8px;
      min-width: 60px;
      text-align: right;
      font-family: var(--mono);
    }
    .header-row span:first-child {
      min-width: 40px;
      text-align: right;
      color: var(--text-muted);
    }
    .scroll-area {
      flex: 1;
      min-height: 200px;
      overflow-y: auto;
      position: relative;
    }
    .spacer {
      width: 1px;
    }
    .rows {
      position: absolute;
      left: 0;
      right: 0;
    }
    .row {
      display: flex;
      height: ${ROW_HEIGHT}px;
      align-items: center;
      font-family: var(--mono);
      font-size: 0.75rem;
      border-bottom: 1px solid var(--bg);
    }
    .row:hover { background: var(--bg-hover); }
    .row.highlight { background: var(--accent-subtle); }
    .row span {
      padding: 0 8px;
      min-width: 60px;
      text-align: right;
      white-space: nowrap;
    }
    .row span:first-child {
      min-width: 40px;
      color: var(--text-muted);
    }
  `;

  static properties = {
    store: { type: Object },
    fields: { type: Array },
    highlightIndices: { type: Object },
  };

  constructor() {
    super();
    this.store = null;
    this.fields = [];
    this.highlightIndices = null;
    this._renderedStart = -1;
    this._renderedCount = 0;
    this._rafId = null;
  }

  updated(changed) {
    if (changed.has('store') || changed.has('fields') || changed.has('highlightIndices')) {
      this.updateComplete.then(() => this._renderRows());
    }
  }

  render() {
    if (!this.store || !this.fields?.length) return '';

    return html`
      <div class="container">
        <div class="header-row">
          <span>#</span>
          ${this.fields.map(f => html`<span>${f}</span>`)}
        </div>
        <div class="scroll-area" @scroll=${this._onScroll}>
          <div class="spacer" style="height:${this._spacerHeight()}px"></div>
          <div class="rows"></div>
        </div>
      </div>
    `;
  }

  /** Spacer height, capped to avoid browser limits. */
  _spacerHeight() {
    if (!this.store) return 0;
    const natural = this.store.entryCount() * ROW_HEIGHT;
    return Math.min(natural, MAX_SPACER);
  }

  /** Is the spacer capped (i.e., scroll position needs remapping)? */
  _isRemapped() {
    if (!this.store) return false;
    return this.store.entryCount() * ROW_HEIGHT > MAX_SPACER;
  }

  /** Convert a scroll position to entry index. */
  _scrollToEntry(scrollTop, scrollEl) {
    if (!this._isRemapped()) {
      return Math.floor(scrollTop / ROW_HEIGHT);
    }
    // Remapped: scroll fraction → entry index
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const frac = scrollTop / maxScroll;
    const totalEntries = this.store.entryCount();
    const maxStart = totalEntries - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    return Math.round(frac * Math.max(0, maxStart));
  }

  /** Convert an entry index to scroll position. */
  _entryToScroll(index, scrollEl) {
    if (!this._isRemapped()) {
      return index * ROW_HEIGHT;
    }
    const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
    if (maxScroll <= 0) return 0;
    const totalEntries = this.store.entryCount();
    const maxStart = totalEntries - Math.ceil(scrollEl.clientHeight / ROW_HEIGHT);
    if (maxStart <= 0) return 0;
    const frac = index / maxStart;
    return Math.round(frac * maxScroll);
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
    if (!scrollEl || !rowsEl || !this.store || !this.fields?.length) return;

    const firstVisible = this._scrollToEntry(scrollEl.scrollTop, scrollEl);
    const containerHeight = scrollEl.clientHeight || 500;
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const startIdx = Math.max(0, firstVisible - OVERSCAN);
    const endIdx = Math.min(this.store.entryCount(), startIdx + visibleCount);
    const count = endIdx - startIdx;

    if (startIdx === this._renderedStart && count === this._renderedCount) return;
    this._renderedStart = startIdx;
    this._renderedCount = count;

    if (count <= 0) {
      rowsEl.innerHTML = '';
      rowsEl.style.top = '0px';
      return;
    }

    let entries;
    try {
      entries = this.store.entriesRange(startIdx, count);
    } catch (err) {
      console.error('Failed to fetch entries:', err);
      return;
    }

    // Position rows at the right place in the scroll area.
    // For remapped mode, position relative to the scroll fraction.
    if (this._isRemapped()) {
      const maxScroll = scrollEl.scrollHeight - scrollEl.clientHeight;
      const totalEntries = this.store.entryCount();
      const maxStart = totalEntries - Math.ceil(containerHeight / ROW_HEIGHT);
      const frac = maxStart > 0 ? startIdx / maxStart : 0;
      rowsEl.style.top = `${Math.round(frac * maxScroll)}px`;
    } else {
      rowsEl.style.top = `${startIdx * ROW_HEIGHT}px`;
    }

    const fields = this.fields;
    const hl = this.highlightIndices;
    const parts = [];
    for (let i = 0; i < entries.length; i++) {
      const idx = startIdx + i;
      const data = entries[i];
      const cls = hl?.has(idx) ? 'row highlight' : 'row';
      parts.push(`<div class="${cls}" data-idx="${idx}">`);
      parts.push(`<span>${idx}</span>`);
      for (const f of fields) {
        parts.push(`<span>${displayVal(data[f])}</span>`);
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

  /** Scroll to a specific entry index. */
  scrollToIndex(index) {
    const scrollEl = this.renderRoot?.querySelector('.scroll-area');
    if (!scrollEl) return;
    this._renderedStart = -1;
    scrollEl.scrollTop = this._entryToScroll(index, scrollEl);
    this._renderRows();
  }
}

customElements.define('trace-table', TraceTable);
