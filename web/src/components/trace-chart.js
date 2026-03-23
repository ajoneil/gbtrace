import { LitElement, html, css } from 'lit';
import { displayVal } from '../lib/format.js';

const CHART_HEIGHT = 174;
const DIFF_LANE_HEIGHT = 6;
const PADDING = { top: 20, right: 12, bottom: 24, left: 50 };

export class TraceChart extends LitElement {
  static styles = css`
    :host { display: block; }
    .chart-wrap {
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
      padding: 8px;
      position: relative;
    }
    .chart-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: 4px;
      font-size: 0.75rem;
    }
    .chart-title {
      font-family: var(--mono);
      font-weight: 600;
      color: var(--accent);
    }
    .chart-range {
      color: var(--text-muted);
    }
    .chart-actions {
      display: flex;
      gap: 4px;
    }
    .chart-btn {
      padding: 2px 8px;
      background: none;
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.7rem;
      font-family: inherit;
    }
    .chart-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    canvas {
      display: block;
      width: 100%;
      cursor: crosshair;
    }
    .tooltip {
      position: absolute;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      padding: 3px 8px;
      font-family: var(--mono);
      font-size: 0.7rem;
      color: var(--text);
      pointer-events: none;
      white-space: nowrap;
      z-index: 10;
    }
  `;

  static properties = {
    store: { type: Object },
    storeB: { type: Object },
    nameA: { type: String },
    nameB: { type: String },
    field: { type: String },
    highlightIndices: { type: Object },
    cursorIndex: { type: Number },
    _viewStart: { state: true },
    _viewEnd: { state: true },
    _tooltip: { state: true },
    _isDragging: { state: true },
  };

  constructor() {
    super();
    this.store = null;
    this.storeB = null;
    this.nameA = '';
    this.nameB = '';
    this.field = null;
    this.highlightIndices = null;
    this.cursorIndex = null;
    this._viewStart = 0;
    this._viewEnd = 0;
    this._tooltip = null;
    this._isDragging = false;
    this._dragStartX = 0;
    this._dragViewStart = 0;
    this._dragViewEnd = 0;
    this._selStart = null;
    this._selEnd = null;
    this._resizeObserver = null;
  }

  connectedCallback() {
    super.connectedCallback();
    this._resizeObserver = new ResizeObserver(() => this._draw());
    this._resizeObserver.observe(this);
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this._resizeObserver?.disconnect();
  }

  updated(changed) {
    if (changed.has('store') || changed.has('storeB') || changed.has('field')) {
      if (this.store && this.field) {
        this._viewStart = 0;
        this._viewEnd = this.store.entryCount();
        this.updateComplete.then(() => this._draw());
      }
    }
    if (changed.has('highlightIndices') || changed.has('cursorIndex') || changed.has('_viewStart') || changed.has('_viewEnd')) {
      this.updateComplete.then(() => this._draw());
    }
  }

  render() {
    if (!this.store || !this.field) return '';
    const total = this.store.entryCount();
    const rangeText = this._viewEnd - this._viewStart < total
      ? `${this._viewStart.toLocaleString()} - ${this._viewEnd.toLocaleString()} of ${total.toLocaleString()}`
      : `${total.toLocaleString()} entries`;

    return html`
      <div class="chart-wrap">
        <div class="chart-header">
          <span class="chart-title">${this.field}</span>
          <span class="chart-range">${rangeText}</span>
          <div class="chart-actions">
            ${this._viewEnd - this._viewStart < total ? html`
              <button class="chart-btn" @click=${this._resetZoom}>Reset</button>
            ` : ''}
          </div>
        </div>
        <canvas
          height="${CHART_HEIGHT}"
          @wheel=${this._onWheel}
          @mousedown=${this._onMouseDown}
          @mousemove=${this._onMouseMove}
          @mouseup=${this._onMouseUp}
          @mouseleave=${this._onMouseLeave}
        ></canvas>
        ${this._tooltip ? html`
          <div class="tooltip" style="left:${this._tooltip.x}px;top:${this._tooltip.y}px">
            #${this._tooltip.index} ${this.field}=${this._tooltip.value}
          </div>
        ` : ''}
      </div>
    `;
  }

  _getCanvas() {
    return this.renderRoot?.querySelector('canvas');
  }

  _draw() {
    const canvas = this._getCanvas();
    if (!canvas || !this.store || !this.field) return;

    const rect = canvas.parentElement.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const cssWidth = rect.width - 16;
    canvas.style.width = `${cssWidth}px`;
    canvas.width = cssWidth * dpr;
    canvas.height = CHART_HEIGHT * dpr;

    const ctx = canvas.getContext('2d');
    ctx.scale(dpr, dpr);

    const w = cssWidth;
    const h = CHART_HEIGHT;
    const hasDiffLane = !!this.storeB;
    const bottomPad = PADDING.bottom + (hasDiffLane ? DIFF_LANE_HEIGHT + 6 : 0);
    const plotW = w - PADDING.left - PADDING.right;
    const plotH = h - PADDING.top - bottomPad;
    ctx.clearRect(0, 0, w, h);

    const start = this._viewStart;
    const end = this._viewEnd;
    const buckets = Math.min(plotW, end - start);
    if (buckets <= 0) return;

    let summaryA;
    try {
      summaryA = this.store.fieldSummary(this.field, start, end, buckets);
    } catch (e) { return; }

    let summaryB = null;
    if (this.storeB) {
      try {
        summaryB = this.storeB.fieldSummary(this.field, start, end, buckets);
      } catch (e) { /* storeB may not have this field */ }
    }

    // Global min/max across both traces
    let yMin = Infinity, yMax = -Infinity;
    for (let i = 0; i < buckets; i++) {
      const mnA = summaryA[i * 2], mxA = summaryA[i * 2 + 1];
      if (mnA < yMin) yMin = mnA;
      if (mxA > yMax) yMax = mxA;
      if (summaryB) {
        const mnB = summaryB[i * 2], mxB = summaryB[i * 2 + 1];
        if (mnB < yMin) yMin = mnB;
        if (mxB > yMax) yMax = mxB;
      }
    }
    if (yMin === yMax) { yMin -= 1; yMax += 1; }

    const toX = (bucket) => PADDING.left + (bucket / buckets) * plotW;
    const toY = (val) => PADDING.top + plotH - ((val - yMin) / (yMax - yMin)) * plotH;

    // Highlights
    if (this.highlightIndices?.size > 0) {
      ctx.fillStyle = 'rgba(88,166,255,0.08)';
      const range = end - start;
      for (const idx of this.highlightIndices) {
        if (idx >= start && idx < end) {
          const x = toX(((idx - start) / range) * buckets);
          ctx.fillRect(x - 0.5, PADDING.top, 1, plotH);
        }
      }
    }

    // Selection
    if (this._selStart !== null && this._selEnd !== null) {
      ctx.fillStyle = 'rgba(88,166,255,0.15)';
      const x1 = Math.min(this._selStart, this._selEnd);
      const x2 = Math.max(this._selStart, this._selEnd);
      ctx.fillRect(x1, PADDING.top, x2 - x1, plotH);
    }

    if (summaryB) {
      // --- Dual trace mode ---
      // Trace A: blue
      this._drawTrace(ctx, summaryA, buckets, toX, toY, 'rgba(88,166,255,0.15)', '#58a6ff');
      // Trace B: yellow
      this._drawTrace(ctx, summaryB, buckets, toX, toY, 'rgba(210,153,34,0.15)', '#d29922');

      // Diff lane: red bar below the plot for each bucket where values differ
      const laneY = PADDING.top + plotH + 4;
      for (let i = 0; i < buckets; i++) {
        const mnA = summaryA[i * 2], mxA = summaryA[i * 2 + 1];
        const mnB = summaryB[i * 2], mxB = summaryB[i * 2 + 1];
        if (mnA !== mnB || mxA !== mxB) {
          const x = toX(i);
          const bw = Math.max(toX(i + 1) - x, 1);
          ctx.fillStyle = '#f85149';
          ctx.fillRect(x, laneY, bw, DIFF_LANE_HEIGHT);
        }
      }
    } else {
      // --- Single trace mode ---
      this._drawTrace(ctx, summaryA, buckets, toX, toY, 'rgba(88,166,255,0.2)', '#58a6ff');
    }

    // Cursor line — draw with contrast backing so it's visible on busy graphs
    if (this.cursorIndex != null && this.cursorIndex >= start && this.cursorIndex < end) {
      const frac = (this.cursorIndex - start) / (end - start);
      const cx = PADDING.left + frac * plotW;

      // Dark backing for contrast
      ctx.strokeStyle = 'rgba(13,17,23,0.7)';
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(cx, PADDING.top);
      ctx.lineTo(cx, PADDING.top + plotH);
      ctx.stroke();

      // Bright orange line on top
      ctx.strokeStyle = '#f0883e';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(cx, PADDING.top);
      ctx.lineTo(cx, PADDING.top + plotH);
      ctx.stroke();

      // Value dots with outline
      const drawDot = (store, color) => {
        const val = store.entry(this.cursorIndex);
        if (!val) return;
        const yv = val[this.field];
        if (yv == null || typeof yv !== 'number') return;
        const cy = toY(yv);
        // Dark outline
        ctx.fillStyle = 'rgba(13,17,23,0.8)';
        ctx.beginPath();
        ctx.arc(cx, cy, 5, 0, Math.PI * 2);
        ctx.fill();
        // Colored dot
        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.arc(cx, cy, 3, 0, Math.PI * 2);
        ctx.fill();
      };

      drawDot(this.store, '#58a6ff');
      if (this.storeB) drawDot(this.storeB, '#d29922');
    }

    // Y axis
    ctx.fillStyle = '#8b949e';
    ctx.font = '10px monospace';
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    const yTicks = 5;
    for (let i = 0; i <= yTicks; i++) {
      const val = yMin + (i / yTicks) * (yMax - yMin);
      const y = toY(val);
      ctx.fillText(displayVal(Math.round(val)), PADDING.left - 4, y);
      ctx.strokeStyle = 'rgba(139,148,158,0.15)';
      ctx.lineWidth = 0.5;
      ctx.beginPath();
      ctx.moveTo(PADDING.left, y);
      ctx.lineTo(w - PADDING.right, y);
      ctx.stroke();
    }

    // X axis
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    const xTicks = Math.min(6, buckets);
    for (let i = 0; i <= xTicks; i++) {
      const idx = start + Math.round((i / xTicks) * (end - start));
      const x = PADDING.left + (i / xTicks) * plotW;
      ctx.fillText(idx.toLocaleString(), x, h - bottomPad + (hasDiffLane ? DIFF_LANE_HEIGHT + 10 : 6));
    }

    // Legend for dual mode
    if (summaryB) {
      ctx.font = '10px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      const lx = PADDING.left + 4;
      const ly = PADDING.top + 2;
      ctx.fillStyle = '#58a6ff';
      ctx.fillRect(lx, ly, 10, 2);
      ctx.fillText(this.nameA || 'A', lx + 14, ly - 4);
      ctx.fillStyle = '#d29922';
      ctx.fillRect(lx, ly + 12, 10, 2);
      ctx.fillText(this.nameB || 'B', lx + 14, ly + 8);
    }
  }

  /** Draw a single trace's area fill + midline. */
  _drawTrace(ctx, summary, buckets, toX, toY, fillColor, strokeColor) {
    ctx.beginPath();
    for (let i = 0; i < buckets; i++) {
      const x = toX(i);
      const y = toY(summary[i * 2 + 1]);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    for (let i = buckets - 1; i >= 0; i--) {
      ctx.lineTo(toX(i), toY(summary[i * 2]));
    }
    ctx.closePath();
    ctx.fillStyle = fillColor;
    ctx.fill();

    ctx.beginPath();
    for (let i = 0; i < buckets; i++) {
      const x = toX(i);
      const mid = (summary[i * 2] + summary[i * 2 + 1]) / 2;
      const y = toY(mid);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = strokeColor;
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  _pixelToIndex(clientX) {
    const canvas = this._getCanvas();
    if (!canvas) return this._viewStart;
    const rect = canvas.getBoundingClientRect();
    const x = clientX - rect.left;
    const plotW = rect.width - PADDING.left - PADDING.right;
    const frac = Math.max(0, Math.min(1, (x - PADDING.left) / plotW));
    return Math.round(this._viewStart + frac * (this._viewEnd - this._viewStart));
  }

  _onWheel(e) {
    e.preventDefault();
    const total = this.store.entryCount();
    const range = this._viewEnd - this._viewStart;
    const idx = this._pixelToIndex(e.clientX);
    const frac = (idx - this._viewStart) / range;

    const zoomFactor = e.deltaY > 0 ? 1.3 : 0.7;
    let newRange = Math.round(range * zoomFactor);
    newRange = Math.max(100, Math.min(total, newRange));

    let newStart = Math.round(idx - frac * newRange);
    newStart = Math.max(0, Math.min(total - newRange, newStart));

    this._viewStart = newStart;
    this._viewEnd = newStart + newRange;
  }

  _onMouseDown(e) {
    if (e.button !== 0) return;
    const canvas = this._getCanvas();
    if (!canvas) return;
    // Start drag — we'll decide if it's a zoom-select or a click on mouseup
    this._isDragging = 'pending';
    this._dragOriginX = e.clientX;
    this._selStart = e.clientX - canvas.getBoundingClientRect().left;
    this._selEnd = this._selStart;
  }

  _onMouseMove(e) {
    const canvas = this._getCanvas();
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();

    if (this._isDragging === 'pending') {
      // Promote to drag-select once moved enough
      if (Math.abs(e.clientX - this._dragOriginX) > 3) {
        this._isDragging = 'select';
      }
    }

    if (this._isDragging === 'select') {
      this._selEnd = e.clientX - rect.left;
      this._tooltip = null;
      this._draw();
      return;
    }

    // Tooltip
    const idx = this._pixelToIndex(e.clientX);
    if (idx >= this._viewStart && idx < this._viewEnd) {
      const val = this.store.entry(idx);
      this._tooltip = {
        x: e.clientX - rect.left + 12,
        y: e.clientY - rect.top - 20,
        index: idx,
        value: val ? displayVal(val[this.field]) : '?',
      };
    }
  }

  _onMouseUp(e) {
    if (this._isDragging === 'select' && this._selStart !== null && this._selEnd !== null) {
      // Drag-to-zoom: zoom into the selected region
      const x1 = Math.min(this._selStart, this._selEnd);
      const x2 = Math.max(this._selStart, this._selEnd);
      if (x2 - x1 > 5) {
        const canvas = this._getCanvas();
        const rect = canvas.getBoundingClientRect();
        const plotW = rect.width - PADDING.left - PADDING.right;
        const frac1 = Math.max(0, (x1 - PADDING.left) / plotW);
        const frac2 = Math.min(1, (x2 - PADDING.left) / plotW);
        const range = this._viewEnd - this._viewStart;
        const newStart = Math.round(this._viewStart + frac1 * range);
        const newEnd = Math.round(this._viewStart + frac2 * range);
        this._viewStart = newStart;
        this._viewEnd = Math.max(newEnd, newStart + 100);
      }
    } else if (this._isDragging === 'pending') {
      // Was a click, not a drag — jump to that index in the trace table
      const idx = this._pixelToIndex(e.clientX);
      this.dispatchEvent(new CustomEvent('jump-to-index', {
        detail: { index: idx },
        bubbles: true, composed: true,
      }));
    }
    this._isDragging = false;
    this._selStart = null;
    this._selEnd = null;
  }

  _onMouseLeave() {
    this._tooltip = null;
    if (this._isDragging === 'select') {
      this._isDragging = false;
      this._selStart = null;
      this._selEnd = null;
      this._draw();
    }
  }

  _resetZoom() {
    this._viewStart = 0;
    this._viewEnd = this.store.entryCount();
  }
}

customElements.define('trace-chart', TraceChart);
