import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const ROW_HEIGHT = 24;
const OVERSCAN = 10;
const MAX_SPACER = 10_000_000;
const COL_WIDTH = 48;
const IDX_WIDTH = 50;
const PC_WIDTH = 48;
const ASM_WIDTH = 100;

export class TraceDiffTable extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .outer {
      display: flex;
      flex: 1;
      min-height: 200px;
      border: 1px solid var(--border);
      border-radius: 8px;
      overflow: hidden;
    }
    .shared {
      flex-shrink: 0;
      overflow: hidden;
      background: var(--bg-surface);
      border-right: 1px solid var(--border);
      position: relative;
    }
    .shared .inner { position: relative; }
    .panels {
      display: flex;
      flex: 1;
      min-width: 0;
    }
    .panel {
      flex: 1;
      overflow: auto;
      background: var(--bg-surface);
      position: relative;
    }
    .panel-a { border-right: 2px solid #58a6ff; }
    .panel-b { border-left: 2px solid #d29922; }
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
    this._pcMatches = true;
  }

  get _visibleFields() {
    return (this.fields || []).filter(f => !this.hiddenFields?.has(f));
  }

  /** Fields shown in the per-side panels (exclude shared fields). */
  get _sideFields() {
    const shared = this._sharedFields;
    return this._visibleFields.filter(f => !shared.has(f));
  }

  /** Fields pulled into the shared left column. */
  get _sharedFields() {
    const s = new Set();
    if (this._pcMatches) {
      if (this._visibleFields.includes('pc')) s.add('pc');
    }
    return s;
  }

  updated(changed) {
    if (changed.has('storeA') || changed.has('storeB') || changed.has('fields') || changed.has('highlightIndices') || changed.has('hiddenFields')) {
      this._renderedStart = -1;
      this._checkPcMatch();
      this.updateComplete.then(() => {
        this._renderRows();
      });
    }
  }

  /** Quick check: do PC values match between traces for the first 1000 entries? */
  _checkPcMatch() {
    if (!this.storeA || !this.storeB) { this._pcMatches = false; return; }
    try {
      const indices = this.storeA.diffField(this.storeB, 'pc');
      // If first diff is beyond 1000, consider them matching
      this._pcMatches = indices.length === 0 || indices[0] > 1000;
    } catch (_) {
      this._pcMatches = false;
    }
  }

  _cs(width, extra = '') {
    return `padding:0 4px;width:${width}px;min-width:${width}px;max-width:${width}px;text-align:right;white-space:nowrap;font-family:var(--mono);font-size:0.7rem;box-sizing:border-box;${extra}`;
  }

  _hdr(width, extra = '') {
    return `${this._cs(width, extra)}padding-top:6px;padding-bottom:6px;color:var(--text-muted);`;
  }

  render() {
    if (!this.storeA || !this.storeB || !this.fields?.length) return '';
    const sf = this._sideFields;
    const shared = this._sharedFields;
    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;
    const showAsm = hasRom && this._pcMatches;

    return html`
      <div class="outer">
        <div class="shared" id="shared-panel">
          <div class="inner">
            <div class="header-row">
              <span style="${this._hdr(IDX_WIDTH)}">#</span>
              ${shared.has('pc') ? html`<span style="${this._hdr(PC_WIDTH)}">pc</span>` : ''}
              ${showAsm ? html`<span style="${this._hdr(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}
            </div>
            <div class="spacer" style="height:${this._spacerHeight()}px"></div>
            <div class="rows" id="rows-shared"></div>
          </div>
        </div>
        <div class="panels">
          <div class="panel panel-a" id="panel-a" @scroll=${this._onScrollA}>
            <div class="inner">
              <div class="header-row">
                ${sf.map(f => {
                  const showAsmHdr = hasRom && !this._pcMatches && f === 'pc';
                  return html`<span style="${this._hdr(COL_WIDTH)}">${f}</span>${showAsmHdr ? html`<span style="${this._hdr(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}`;
                })}
              </div>
              <div class="spacer" style="height:${this._spacerHeight()}px"></div>
              <div class="rows" id="rows-a"></div>
            </div>
          </div>
          <div class="panel panel-b" id="panel-b" @scroll=${this._onScrollB}>
            <div class="inner">
              <div class="header-row">
                ${sf.map(f => {
                  const showAsmHdr = hasRom && !this._pcMatches && f === 'pc';
                  return html`<span style="${this._hdr(COL_WIDTH)}">${f}</span>${showAsmHdr ? html`<span style="${this._hdr(ASM_WIDTH, 'text-align:left;')}">asm</span>` : ''}`;
                })}
              </div>
              <div class="spacer" style="height:${this._spacerHeight()}px"></div>
              <div class="rows" id="rows-b"></div>
            </div>
          </div>
        </div>
      </div>
    `;
  }

  // Hmm, the header for A/B panels is wrong - the first field name is replaced by the emu name.
  // Let me fix the render to show all side fields with a proper header.

  _onScrollA(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelB = this.renderRoot?.querySelector('#panel-b');
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (panelB) {
      panelB.scrollTop = e.target.scrollTop;
      panelB.scrollLeft = e.target.scrollLeft;
    }
    if (shared) shared.scrollTop = e.target.scrollTop;
    this._syncing = false;
    this._scheduleRender();
  }

  _onScrollB(e) {
    if (this._syncing) return;
    this._syncing = true;
    const panelA = this.renderRoot?.querySelector('#panel-a');
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (panelA) {
      panelA.scrollTop = e.target.scrollTop;
      panelA.scrollLeft = e.target.scrollLeft;
    }
    if (shared) shared.scrollTop = e.target.scrollTop;
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
    const rowsShared = this.renderRoot?.querySelector('#rows-shared');
    if (!panelA || !rowsA || !rowsB || !rowsShared || !this.storeA || !this.storeB) return;

    const sf = this._sideFields;
    const shared = this._sharedFields;
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
      rowsA.innerHTML = ''; rowsB.innerHTML = ''; rowsShared.innerHTML = '';
      rowsA.style.top = rowsB.style.top = rowsShared.style.top = '0px';
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
    rowsShared.style.top = `${top}px`;

    const hasRom = (this.storeA.hasRom?.() || this.storeB.hasRom?.()) ?? false;
    const showSharedAsm = hasRom && this._pcMatches;
    const showSideAsm = hasRom && !this._pcMatches;
    let disasmArr = null;  // shared disasm (when PCs match)
    let disasmA = null;    // per-side disasm (when PCs diverge)
    let disasmB = null;
    if (showSharedAsm) {
      const ds = this.storeA.hasRom?.() ? this.storeA : this.storeB;
      try { disasmArr = ds.disassembleRange(startIdx, count); } catch (_) {}
    } else if (showSideAsm) {
      if (this.storeA.hasRom?.()) try { disasmA = this.storeA.disassembleRange(startIdx, count); } catch (_) {}
      if (this.storeB.hasRom?.()) try { disasmB = this.storeB.disassembleRange(startIdx, count); } catch (_) {}
    }

    const cs = this._cs.bind(this);
    const hl = this.highlightIndices;
    const partsShared = [];
    const partsA = [];
    const partsB = [];

    for (let i = 0; i < entriesA.length; i++) {
      const idx = startIdx + i;
      const a = entriesA[i];
      const b = entriesB[i];

      let anyDiff = false;
      for (const f of sf) {
        if (a[f] !== b[f]) { anyDiff = true; break; }
      }
      // Also check shared fields for diff highlighting
      for (const f of shared) {
        if (a[f] !== b[f]) { anyDiff = true; break; }
      }

      const hlBg = hl?.has(idx) ? 'background:var(--accent-subtle);' : '';
      const diffBg = anyDiff ? 'background:rgba(248,81,73,0.06);' : '';
      const bg = hlBg || diffBg;
      const rowStart = `<div data-idx="${idx}" style="display:flex;height:${ROW_HEIGHT}px;align-items:center;border-bottom:1px solid var(--bg);${bg}">`;

      // Shared column
      partsShared.push(rowStart);
      partsShared.push(`<span style="${cs(IDX_WIDTH, 'color:var(--text-muted);')}">${idx}</span>`);
      // cy removed from format
      if (shared.has('pc')) {
        const pcDiff = a.pc !== b.pc;
        partsShared.push(`<span style="${cs(PC_WIDTH, pcDiff ? 'color:var(--red);' : '')}">${displayVal(a.pc, 'pc')}</span>`);
      }
      if (disasmArr) {
        partsShared.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmArr[i] || ''}</span>`);
      }
      partsShared.push('</div>');

      // Panel A
      partsA.push(rowStart);
      for (const f of sf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--red);font-weight:600;' : '';
        partsA.push(`<span style="${cs(COL_WIDTH, color)}">${displayVal(a[f], f)}</span>`);
        if (disasmA && f === 'pc') {
          partsA.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmA[i] || ''}</span>`);
        }
      }
      partsA.push('</div>');

      // Panel B
      partsB.push(rowStart);
      for (const f of sf) {
        const differs = a[f] !== b[f];
        const color = differs ? 'color:var(--yellow);font-weight:600;' : '';
        partsB.push(`<span style="${cs(COL_WIDTH, color)}">${displayVal(b[f], f)}</span>`);
        if (disasmB && f === 'pc') {
          partsB.push(`<span style="${cs(ASM_WIDTH, 'text-align:left;color:var(--green);')}">${disasmB[i] || ''}</span>`);
        }
      }
      partsB.push('</div>');
    }

    rowsShared.innerHTML = partsShared.join('');
    rowsA.innerHTML = partsA.join('');
    rowsB.innerHTML = partsB.join('');

    for (const rows of [rowsShared, rowsA, rowsB]) {
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
    const shared = this.renderRoot?.querySelector('#shared-panel');
    if (!panelA) return;
    this._renderedStart = -1;
    const scrollTop = this._entryToScroll(index, panelA);
    panelA.scrollTop = scrollTop;
    if (panelB) panelB.scrollTop = scrollTop;
    if (shared) shared.scrollTop = scrollTop;
    this._renderRows();
  }
}

customElements.define('trace-diff-table', TraceDiffTable);
