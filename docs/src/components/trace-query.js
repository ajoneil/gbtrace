import { LitElement, html, css } from 'lit';
import { displayVal, normalizeInput } from '../lib/format.js';

const SEMANTIC_CONDITIONS = [
  // PPU
  { group: 'PPU', label: 'HBlank', query: 'ppu enters mode 0', needs: 'stat' },
  { group: 'PPU', label: 'VBlank', query: 'ppu enters mode 1', needs: 'stat' },
  { group: 'PPU', label: 'OAM Scan', query: 'ppu enters mode 2', needs: 'stat' },
  { group: 'PPU', label: 'Drawing', query: 'ppu enters mode 3', needs: 'stat' },
  { group: 'PPU', label: 'LCD On', query: 'lcd on', needs: 'lcdc' },
  { group: 'PPU', label: 'LCD Off', query: 'lcd off', needs: 'lcdc' },
  // Interrupts
  { group: 'IRQ', label: 'VBlank', query: 'interrupt 0', needs: 'if_' },
  { group: 'IRQ', label: 'STAT', query: 'interrupt 1', needs: 'if_' },
  { group: 'IRQ', label: 'Timer', query: 'interrupt 2', needs: 'if_' },
  { group: 'IRQ', label: 'Serial', query: 'interrupt 3', needs: 'if_' },
  { group: 'IRQ', label: 'Joypad', query: 'interrupt 4', needs: 'if_' },
  // Timer
  { group: 'Timer', label: 'Overflow', query: 'timer overflow', needs: 'tima' },
];

// Flag chips cycle: off -> becomes set -> becomes clear -> off
const FLAG_CHIPS = [
  { name: 'Z', flag: 'z' },
  { name: 'N', flag: 'n' },
  { name: 'H', flag: 'h' },
  { name: 'C', flag: 'c' },
];
const FLAG_MODES = [null, 'set', 'clear'];  // cycle order

export class TraceQuery extends LitElement {
  static styles = css`
    :host { display: block; }

    .section-label {
      font-size: 0.7rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
      padding: 4px 0;
      white-space: nowrap;
    }
    .chip-row {
      display: flex;
      flex-wrap: wrap;
      gap: 5px;
      align-items: center;
      margin-bottom: 6px;
    }
    .chip {
      padding: 3px 9px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 10px;
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
    .chip.selected {
      border-color: var(--yellow);
      color: var(--yellow);
    }

    /* --- Field detail bar --- */
    .field-detail {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      margin-bottom: 12px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      font-size: 0.8rem;
    }
    .field-detail .field-name {
      font-family: var(--mono);
      font-weight: 600;
      color: var(--accent);
    }
    .field-detail .op-chip {
      padding: 3px 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 10px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.75rem;
    }
    .field-detail .op-chip:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .field-detail .op-chip.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }
    .field-detail input {
      width: 100px;
      padding: 3px 8px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 6px;
      color: var(--text);
      font-family: var(--mono);
      font-size: 0.8rem;
    }
    .field-detail input:focus {
      outline: none;
      border-color: var(--accent);
    }
    .field-detail input::placeholder { color: var(--text-muted); }
    .field-detail .close-btn {
      margin-left: auto;
      padding: 2px 8px;
      background: none;
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.75rem;
    }
    .field-detail .close-btn:hover {
      border-color: var(--red);
      color: var(--red);
    }

    /* --- Results --- */
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
    .result-field { color: var(--text); }
    .result-field .fname { color: var(--text-muted); }

    .error { color: var(--red); margin-top: 8px; font-size: 0.8rem; }
    .truncated {
      padding: 6px 12px;
      font-size: 0.75rem;
      color: var(--text-muted);
      text-align: center;
    }
  `;

  static properties = {
    store: { type: Object },
    storeB: { type: Object },
    compareMode: { type: Boolean },
    fields: { type: Array },
    _selectedField: { state: true },
    _fieldOp: { state: true },
    _fieldValue: { state: true },
    _activeQuery: { state: true },
    _activeLabel: { state: true },
    _matches: { state: true },
    _matchEntries: { state: true },
    _currentMatch: { state: true },
    _error: { state: true },
    _flagModes: { state: true },
  };

  constructor() {
    super();
    this.store = null;
    this.storeB = null;
    this.compareMode = false;
    this.fields = [];
    this._selectedField = null;
    this._fieldOp = null;
    this._fieldValue = '';
    this._activeQuery = null;
    this._activeLabel = null;
    this._matches = null;
    this._flagModes = {}; // { z: null, n: null, h: null, c: null }
    this._matchEntries = [];
    this._currentMatch = -1;
    this._error = null;
  }

  render() {
    const traceFields = this.fields || [];
    const semanticAvailable = SEMANTIC_CONDITIONS.filter(c =>
      (this.fields || []).includes(c.needs)
    );

    return html`
      <div class="chip-row">
        <span class="section-label" style="font-weight:600;font-size:0.75rem">Search</span>
      </div>
      ${semanticAvailable.length > 0 ? html`
        <div class="chip-row">
          ${this._renderSemanticInline(semanticAvailable)}
        </div>
      ` : ''}

      <div class="chip-row">
        <span class="section-label">Fields</span>
        ${traceFields.map(f => html`
          <span
            class="chip ${this._selectedField === f ? 'selected' : ''} ${this._activeField === f ? 'active' : ''}"
            @click=${() => this._selectField(f)}
          >${f}</span>
        `)}
        ${(this.fields || []).includes('f') ? html`
          <span class="section-label" style="margin-left:6px">Flags</span>
          ${FLAG_CHIPS.map(fc => {
            const mode = this._flagModes[fc.flag] || null;
            const label = mode ? `${fc.name} ${mode}` : fc.name;
            return html`
              <span
                class="chip ${mode ? 'active' : ''}"
                @click=${() => this._cycleFlag(fc.flag)}
              >${label}</span>
            `;
          })}
        ` : ''}
      </div>

      ${this._selectedField ? html`
        <div class="field-detail">
          <span class="field-name">${this._selectedField}</span>
          ${this.compareMode ? html`
            <span
              class="op-chip ${this._fieldOp === 'differs' ? 'active' : ''}"
              @click=${() => this._runFieldOp('differs')}
            >differs</span>
          ` : ''}
          <span
            class="op-chip ${this._fieldOp === 'changes' ? 'active' : ''}"
            @click=${() => this._runFieldOp('changes')}
          >changes</span>
          <span
            class="op-chip ${this._fieldOp === 'equals' ? 'active' : ''}"
            @click=${() => this._runFieldOp('equals')}
          >equals</span>
          <span
            class="op-chip ${this._fieldOp === 'changes_to' ? 'active' : ''}"
            @click=${() => this._runFieldOp('changes_to')}
          >changes to</span>
          <input
            type="text"
            placeholder="value"
            .value=${this._fieldValue}
            @input=${e => this._fieldValue = e.target.value}
            @keydown=${e => {
              if (e.key === 'Enter') {
                this._runFieldOp(this._fieldValue ? (this._fieldOp || 'equals') : 'changes');
              }
            }}
          >
          <button class="close-btn" @click=${() => this._closeField()}>x</button>
        </div>
      ` : ''}

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
                <span class="result-cy">#${this._matches[i]}</span>
                <span class="result-fields">
                  ${this._summaryFields(entry)}
                </span>
              </div>
            `)}
            ${this._matches.length > this._matchEntries.length ? html`
              <div class="truncated">
                ... and ${(this._matches.length - this._matchEntries.length).toLocaleString()} more
              </div>
            ` : ''}
          </div>
        ` : ''}
      ` : ''}
    `;
  }

  _renderSemanticInline(conditions) {
    const groups = [...new Set(conditions.map(c => c.group))];
    return groups.map((group, i) => {
      const items = conditions.filter(c => c.group === group);
      return html`
        <span class="section-label" style="${i > 0 ? 'margin-left:6px' : ''}">${group}</span>
        ${items.map(c => html`
          <span
            class="chip ${this._activeQuery === c.query ? 'active' : ''}"
            @click=${() => this._toggleSemantic(c.query, c.label)}
          >${c.label}</span>
        `)}
      `;
    });
  }

  get _activeField() {
    if (!this._activeQuery || !this._selectedField) return null;
    if (this._activeQuery.startsWith(this._selectedField + ' ') ||
        this._activeQuery.startsWith(this._selectedField + '=')) {
      return this._selectedField;
    }
    return null;
  }

  _summaryFields(entry) {
    // Show the searched field first, then all trace fields (excluding cy)
    const traceFields = (this.fields || []).filter(f => entry[f] !== undefined);

    // Put the searched field at the front if there is one
    const searchedField = this._selectedField;
    if (searchedField && traceFields.includes(searchedField)) {
      const idx = traceFields.indexOf(searchedField);
      traceFields.splice(idx, 1);
      traceFields.unshift(searchedField);
    }

    return traceFields.map(f =>
      html`<span class="result-field"><span class="fname">${f}</span>=${displayVal(entry[f])}</span>`
    );
  }

  _cycleFlag(flag) {
    const current = this._flagModes[flag] || null;
    const idx = FLAG_MODES.indexOf(current);
    const next = FLAG_MODES[(idx + 1) % FLAG_MODES.length];

    this._flagModes = { ...this._flagModes, [flag]: next };

    if (next) {
      const query = `flag ${flag} becomes ${next}`;
      const label = `flag ${flag.toUpperCase()} becomes ${next}`;
      this._runQuery(query, label);
    } else {
      this._clear();
    }
  }

  _selectField(field) {
    const defaultOp = this.compareMode ? 'differs' : 'changes';
    if (this._selectedField === field && this._fieldOp === defaultOp) {
      this._closeField();
      this._clear();
      this._emitFieldSelected(null);
    } else {
      this._selectedField = field;
      this._fieldValue = '';
      this._emitFieldSelected(field);
      this._runFieldOp(defaultOp);
    }
  }

  _closeField() {
    this._selectedField = null;
    this._fieldOp = null;
    this._fieldValue = '';
    this._emitFieldSelected(null);
  }

  _runFieldOp(op) {
    const field = this._selectedField;
    if (!field) return;

    this._fieldOp = op;
    const val = normalizeInput(this._fieldValue);
    const displayV = displayVal(val);
    let query, label;

    if (op === 'differs') {
      // Compare mode: find entries where field differs between traces
      this._runDiffQuery(field);
      return;
    }

    switch (op) {
      case 'changes':
        query = `${field} changes`;
        label = `${field} changes`;
        break;
      case 'equals':
        if (!val) return;
        query = `${field}=${val}`;
        label = `${field} = ${displayV}`;
        break;
      case 'changes_to':
        if (!val) return;
        query = `${field} changes to ${val}`;
        label = `${field} changes to ${displayV}`;
        break;
      default:
        return;
    }

    this._runQuery(query, label);
  }

  _toggleSemantic(query, label) {
    if (this._activeQuery === query) {
      this._clear();
    } else {
      this._selectedField = null;
      this._fieldOp = null;
      this._fieldValue = '';
      this._flagModes = {};
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
    this._flagModes = {};
    this._emitHighlight();
  }

  _runDiffQuery(field) {
    if (!this.store || !this.storeB) return;
    this._error = null;
    this._activeQuery = `diff:${field}`;
    this._activeLabel = `${field} differs`;
    this._matches = null;
    this._matchEntries = [];
    this._currentMatch = -1;

    try {
      // Run the comparison entirely in WASM — no JS-side entry deserialization
      this._matches = this.store.diffField(this.storeB, field);

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

  _emitFieldSelected(field) {
    this.dispatchEvent(new CustomEvent('field-selected', {
      detail: { field },
      bubbles: true, composed: true,
    }));
  }
}

customElements.define('trace-query', TraceQuery);
