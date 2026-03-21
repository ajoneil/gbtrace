import { LitElement, html, css } from 'lit';

const SEMANTIC_CONDITIONS = [
  { label: 'HBlank', query: 'ppu enters mode 0', needs: 'stat' },
  { label: 'VBlank', query: 'ppu enters mode 1', needs: 'stat' },
  { label: 'OAM Scan', query: 'ppu enters mode 2', needs: 'stat' },
  { label: 'Drawing', query: 'ppu enters mode 3', needs: 'stat' },
  { label: 'LCD On', query: 'lcd on', needs: 'lcdc' },
  { label: 'LCD Off', query: 'lcd off', needs: 'lcdc' },
  { label: 'Timer Overflow', query: 'timer overflow', needs: 'tima' },
  { label: 'VBlank IRQ', query: 'interrupt 0', needs: 'if_' },
  { label: 'STAT IRQ', query: 'interrupt 1', needs: 'if_' },
  { label: 'Timer IRQ', query: 'interrupt 2', needs: 'if_' },
];

export class TraceQuery extends LitElement {
  static styles = css`
    :host { display: block; }

    .section-label {
      font-size: 0.7rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
      margin-bottom: 6px;
    }
    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-bottom: 12px;
    }
    .chip {
      padding: 5px 12px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 14px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.78rem;
      font-family: var(--mono);
      white-space: nowrap;
      user-select: none;
      transition: all 0.15s;
    }
    .chip:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .chip.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
      font-weight: 600;
    }

    .results-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: 6px;
    }
    .results-header .count {
      font-size: 0.8rem;
      color: var(--text-muted);
    }
    .results-header .count strong {
      color: var(--accent);
    }
    .results-header .clear-btn {
      font-size: 0.75rem;
      color: var(--text-muted);
      cursor: pointer;
      background: none;
      border: 1px solid var(--border);
      border-radius: 4px;
      padding: 2px 8px;
    }
    .results-header .clear-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }

    .results-list {
      max-height: 240px;
      overflow-y: auto;
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
    }
    .result-item {
      display: flex;
      align-items: center;
      padding: 6px 12px;
      font-family: var(--mono);
      font-size: 0.75rem;
      cursor: pointer;
      border-bottom: 1px solid var(--bg);
      gap: 12px;
    }
    .result-item:last-child { border-bottom: none; }
    .result-item:hover { background: var(--bg-hover); }
    .result-item.current { background: var(--accent-subtle); }
    .result-idx {
      color: var(--text-muted);
      min-width: 50px;
      text-align: right;
    }
    .result-cy {
      color: var(--text-muted);
      min-width: 80px;
    }
    .result-fields {
      display: flex;
      gap: 8px;
      flex-wrap: wrap;
    }
    .result-field {
      color: var(--text);
    }
    .result-field .fname {
      color: var(--text-muted);
    }

    .error { color: var(--red); margin-top: 8px; font-size: 0.8rem; }
  `;

  static properties = {
    store: { type: Object },
    fields: { type: Array },
    _activeQuery: { state: true },
    _activeLabel: { state: true },
    _matches: { state: true },
    _matchEntries: { state: true },
    _currentMatch: { state: true },
    _error: { state: true },
  };

  constructor() {
    super();
    this.store = null;
    this.fields = [];
    this._activeQuery = null;
    this._activeLabel = null;
    this._matches = null;
    this._matchEntries = [];
    this._currentMatch = -1;
    this._error = null;
  }

  render() {
    const traceFields = (this.fields || []).filter(f => f !== 'cy');
    const semanticAvailable = SEMANTIC_CONDITIONS.filter(c =>
      (this.fields || []).includes(c.needs)
    );

    return html`
      ${semanticAvailable.length > 0 ? html`
        <div class="section-label">Events</div>
        <div class="chips">
          ${semanticAvailable.map(c => html`
            <span
              class="chip ${this._activeQuery === c.query ? 'active' : ''}"
              @click=${() => this._toggle(c.query, c.label)}
            >${c.label}</span>
          `)}
        </div>
      ` : ''}

      <div class="section-label">Field changes</div>
      <div class="chips">
        ${traceFields.map(f => {
          const q = `${f} changes`;
          return html`
            <span
              class="chip ${this._activeQuery === q ? 'active' : ''}"
              @click=${() => this._toggle(q, `${f} changes`)}
            >${f}</span>
          `;
        })}
      </div>

      ${this._error ? html`<p class="error">${this._error}</p>` : ''}

      ${this._matches ? html`
        <div class="results-header">
          <span class="count">
            <strong>${this._matches.length.toLocaleString()}</strong> matches
            for "${this._activeLabel}"
          </span>
          <button class="clear-btn" @click=${this._clear}>Clear</button>
        </div>
        ${this._matches.length > 0 ? html`
          <div class="results-list">
            ${this._matchEntries.map((entry, i) => html`
              <div
                class="result-item ${i === this._currentMatch ? 'current' : ''}"
                @click=${() => this._jumpTo(i)}
              >
                <span class="result-idx">#${this._matches[i]}</span>
                <span class="result-cy">cy=${entry.cy ?? '?'}</span>
                <span class="result-fields">
                  ${this._summaryFields(entry)}
                </span>
              </div>
            `)}
          </div>
        ` : ''}
      ` : ''}
    `;
  }

  _summaryFields(entry) {
    // Show a few key fields inline
    const show = ['pc', 'a', 'f', 'sp', 'stat', 'ly', 'lcdc', 'if_'];
    const available = show.filter(f => (this.fields || []).includes(f) && entry[f] !== undefined);
    return available.slice(0, 5).map(f =>
      html`<span class="result-field"><span class="fname">${f}</span>=${entry[f]}</span>`
    );
  }

  _toggle(query, label) {
    if (this._activeQuery === query) {
      this._clear();
    } else {
      this._runQuery(query, label);
    }
  }

  _runQuery(queryStr, label) {
    if (!this.store) return;
    this._error = null;
    this._activeQuery = queryStr;
    this._activeLabel = label;
    this._matches = null;
    this._matchEntries = [];
    this._currentMatch = -1;

    try {
      this._matches = this.store.query(queryStr);

      // Fetch entry data for the results list (cap at 500 to avoid OOM)
      const cap = Math.min(this._matches.length, 500);
      const entries = [];
      for (let i = 0; i < cap; i++) {
        entries.push(this.store.entry(this._matches[i]));
      }
      this._matchEntries = entries;

      if (this._matches.length > 0) {
        this._currentMatch = 0;
        this._emitHighlight();
        this._emitJump(this._matches[0]);
      } else {
        this._emitHighlight();
      }
    } catch (err) {
      this._error = `${err.message || err}`;
      this._activeQuery = null;
      this._activeLabel = null;
    }
  }

  _jumpTo(matchIndex) {
    this._currentMatch = matchIndex;
    this._emitJump(this._matches[matchIndex]);
  }

  _clear() {
    this._activeQuery = null;
    this._activeLabel = null;
    this._matches = null;
    this._matchEntries = [];
    this._currentMatch = -1;
    this._error = null;
    this._emitHighlight();
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
