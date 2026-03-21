import { LitElement, html, css } from 'lit';

export class TraceQuery extends LitElement {
  static styles = css`
    :host { display: block; }
    .query-bar {
      display: flex;
      gap: 8px;
      align-items: center;
    }
    input {
      flex: 1;
      padding: 8px 12px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text);
      font-family: var(--mono);
      font-size: 0.85rem;
    }
    input:focus {
      outline: none;
      border-color: var(--accent);
    }
    input::placeholder { color: var(--text-muted); }
    button {
      padding: 8px 16px;
      background: var(--accent-subtle);
      border: 1px solid var(--accent);
      border-radius: 6px;
      color: var(--accent);
      cursor: pointer;
      font-size: 0.85rem;
      white-space: nowrap;
    }
    button:hover { background: var(--accent); color: var(--bg); }
    .results {
      margin-top: 8px;
      font-size: 0.8rem;
      color: var(--text-muted);
      display: flex;
      align-items: center;
      gap: 12px;
    }
    .results .count { color: var(--accent); }
    .nav-btn {
      padding: 2px 8px;
      background: none;
      border: 1px solid var(--border);
      color: var(--text-muted);
      border-radius: 4px;
      cursor: pointer;
      font-size: 0.8rem;
    }
    .nav-btn:hover { border-color: var(--accent); color: var(--accent); }
    .error { color: var(--red); margin-top: 8px; font-size: 0.8rem; }
    .hint {
      margin-top: 6px;
      font-size: 0.75rem;
      color: var(--text-muted);
    }
  `;

  static properties = {
    store: { type: Object },
    _query: { state: true },
    _matches: { state: true },  // Uint32Array
    _currentMatch: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this.store = null;
    this._query = '';
    this._matches = null;
    this._currentMatch = -1;
    this._error = null;
  }

  render() {
    return html`
      <div class="query-bar">
        <input
          type="text"
          placeholder="e.g. pc=0x0150, a changes to 0xFF, ppu enters mode 3"
          .value=${this._query}
          @input=${e => this._query = e.target.value}
          @keydown=${e => { if (e.key === 'Enter') this._search(); }}
        >
        <button @click=${this._search}>Search</button>
        <button @click=${this._clear}>Clear</button>
      </div>
      ${this._error ? html`<p class="error">${this._error}</p>` : ''}
      ${this._matches ? html`
        <div class="results">
          <span class="count">${this._matches.length.toLocaleString()}</span> matches
          ${this._matches.length > 0 ? html`
            <button class="nav-btn" @click=${this._prevMatch}>&lt; Prev</button>
            <span>${this._currentMatch + 1} / ${this._matches.length}</span>
            <button class="nav-btn" @click=${this._nextMatch}>Next &gt;</button>
          ` : ''}
        </div>
      ` : html`
        <p class="hint">
          Conditions: field=value, field changes, field changes to value,
          ppu enters mode N, lcd on/off, interrupt N
        </p>
      `}
    `;
  }

  _search() {
    if (!this.store || !this._query.trim()) return;
    this._error = null;
    this._matches = null;
    this._currentMatch = -1;

    try {
      this._matches = this.store.query(this._query.trim());
      if (this._matches.length > 0) {
        this._currentMatch = 0;
        this._emitHighlight();
        this._emitJump(this._matches[0]);
      } else {
        this._emitHighlight();
      }
    } catch (err) {
      this._error = `${err.message || err}`;
    }
  }

  _clear() {
    this._query = '';
    this._matches = null;
    this._currentMatch = -1;
    this._error = null;
    this._emitHighlight();
  }

  _prevMatch() {
    if (!this._matches?.length) return;
    this._currentMatch = (this._currentMatch - 1 + this._matches.length) % this._matches.length;
    this._emitJump(this._matches[this._currentMatch]);
  }

  _nextMatch() {
    if (!this._matches?.length) return;
    this._currentMatch = (this._currentMatch + 1) % this._matches.length;
    this._emitJump(this._matches[this._currentMatch]);
  }

  _emitHighlight() {
    const indices = this._matches ? new Set(Array.from(this._matches)) : null;
    this.dispatchEvent(new CustomEvent('highlight-changed', {
      detail: { indices },
      bubbles: true, composed: true,
    }));
  }

  _emitJump(index) {
    this.dispatchEvent(new CustomEvent('jump-to-index', {
      detail: { index },
      bubbles: true, composed: true,
    }));
  }
}

customElements.define('trace-query', TraceQuery);
