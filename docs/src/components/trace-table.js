import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;

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
    highlightIndices: { type: Object },  // Set<number>
    _scrollTop: { state: true },
    _visibleRows: { state: true },
  };

  constructor() {
    super();
    this.store = null;
    this.fields = [];
    this.highlightIndices = null;
    this._scrollTop = 0;
    this._visibleRows = [];
  }

  updated(changed) {
    if (changed.has('store') || changed.has('fields')) {
      this._updateVisibleRows();
    }
  }

  render() {
    if (!this.store || !this.fields?.length) return '';
    const totalHeight = this.store.entryCount() * ROW_HEIGHT;

    return html`
      <div class="container">
        <div class="header-row">
          <span>#</span>
          ${this.fields.map(f => html`<span>${f}</span>`)}
        </div>
        <div class="scroll-area" @scroll=${this._onScroll}>
          <div class="spacer" style="height:${totalHeight}px"></div>
          <div class="rows" style="top:${this._rowsTop}px">
            ${this._visibleRows.map(r => html`
              <div class="row ${this._isHighlighted(r.index) ? 'highlight' : ''}">
                <span>${r.index}</span>
                ${this.fields.map(f => html`<span>${displayVal(r.data[f])}</span>`)}
              </div>
            `)}
          </div>
        </div>
      </div>
    `;
  }

  get _rowsTop() {
    if (!this._visibleRows.length) return 0;
    return this._visibleRows[0].index * ROW_HEIGHT;
  }

  _isHighlighted(index) {
    return this.highlightIndices?.has(index) ?? false;
  }

  _onScroll(e) {
    this._scrollTop = e.target.scrollTop;
    this._updateVisibleRows();
  }

  _updateVisibleRows() {
    if (!this.store) { this._visibleRows = []; return; }

    const scrollEl = this.renderRoot?.querySelector('.scroll-area');
    const containerHeight = scrollEl?.clientHeight || 500;
    const startIdx = Math.max(0, Math.floor(this._scrollTop / ROW_HEIGHT) - OVERSCAN);
    const visibleCount = Math.ceil(containerHeight / ROW_HEIGHT) + OVERSCAN * 2;
    const endIdx = Math.min(this.store.entryCount(), startIdx + visibleCount);

    const count = endIdx - startIdx;
    if (count <= 0) { this._visibleRows = []; return; }

    try {
      const entries = this.store.entriesRange(startIdx, count);
      this._visibleRows = entries.map((data, i) => ({
        index: startIdx + i,
        data,
      }));
    } catch (err) {
      console.error('Failed to fetch entries:', err);
      this._visibleRows = [];
    }
  }

  /** Scroll to a specific entry index. */
  scrollToIndex(index) {
    const scrollArea = this.renderRoot?.querySelector('.scroll-area');
    if (scrollArea) {
      scrollArea.scrollTop = index * ROW_HEIGHT;
      // Force update in case scroll event doesn't fire (e.g. same position)
      this._scrollTop = scrollArea.scrollTop;
      this._updateVisibleRows();
    }
  }
}

customElements.define('trace-table', TraceTable);
