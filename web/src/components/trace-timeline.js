import { LitElement, html, css } from 'lit';

/**
 * Timeline selector for navigating trace data.
 *
 * Shows a horizontal bar representing the full trace. Frame boundaries
 * are drawn as tick marks. The user can:
 * - Click a frame to select it
 * - Click+drag on the bar to select any arbitrary range
 * - Use frame step buttons to navigate by frame
 * - Click "All" to view the full trace
 *
 * Emits `view-range-changed` with {start, end} entry indices.
 */
export class TraceTimeline extends LitElement {
  static properties = {
    entryCount: { type: Number },
    frameBoundaries: { type: Array },
    frameBoundariesB: { type: Array },
    viewStart: { type: Number },
    viewEnd: { type: Number },
    compareMode: { type: Boolean },
    entryCountB: { type: Number },
    syncMode: { type: String },
    currentIndex: { type: Number },
    _dragging: { state: true },
  };

  static styles = css`
    :host { display: block; }

    .timeline {
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 8px 12px;
      font-size: 0.8rem;
    }

    .controls {
      display: flex;
      align-items: center;
      gap: 8px;
      margin-bottom: 6px;
      flex-wrap: wrap;
    }

    .frame-nav {
      display: flex;
      align-items: center;
      gap: 4px;
    }

    button {
      padding: 2px 8px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-family: var(--mono);
      font-size: 0.75rem;
    }
    button:hover:not(:disabled) {
      border-color: var(--accent);
      color: var(--accent);
    }
    button:disabled {
      opacity: 0.4;
      cursor: default;
    }
    button.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }

    .frame-label {
      color: var(--text-muted);
      font-family: var(--mono);
      font-size: 0.75rem;
    }
    .frame-label strong {
      color: var(--text);
      font-weight: 600;
    }

    .range-info {
      color: var(--text-muted);
      font-family: var(--mono);
      font-size: 0.72rem;
      margin-left: auto;
    }

    .sync-controls {
      display: flex;
      align-items: center;
      gap: 4px;
      margin-left: 8px;
      padding-left: 8px;
      border-left: 1px solid var(--border);
    }
    .sync-label {
      color: var(--text-muted);
      font-size: 0.7rem;
      font-family: var(--sans);
    }
    .sync-btn {
      padding: 1px 6px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-family: var(--mono);
      font-size: 0.7rem;
    }
    .sync-btn:hover { border-color: var(--accent); color: var(--accent); }
    .sync-btn.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }

    .bar-container {
      position: relative;
      height: 28px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      overflow: hidden;
      cursor: crosshair;
      user-select: none;
    }

    /* Frame boundary tick marks */
    .frame-tick {
      position: absolute;
      top: 0;
      width: 1px;
      height: 100%;
      background: var(--border);
      pointer-events: none;
    }

    /* Selection highlight */
    .selection {
      position: absolute;
      top: 0;
      height: 100%;
      background: var(--accent-subtle);
      pointer-events: none;
    }
    .selection-edge-left, .selection-edge-right {
      position: absolute;
      top: 0;
      width: 2px;
      height: 100%;
      background: var(--accent);
      pointer-events: none;
    }

    .scrubber-row {
      display: flex;
      align-items: center;
      gap: 4px;
      margin: 6px 0 0 0;
    }
    .scrubber {
      flex: 1;
      accent-color: var(--accent);
    }
    .scrub-step {
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-size: 0.75rem;
      padding: 1px 6px;
      font-family: var(--mono);
      line-height: 1.2;
    }
    .scrub-step:hover { border-color: var(--accent); color: var(--accent); }
  `;

  constructor() {
    super();
    this.entryCount = 0;
    this.entryCountB = 0;
    this.frameBoundaries = [];
    this.frameBoundariesB = [];
    this.viewStart = 0;
    this.viewEnd = 0;
    this.compareMode = false;
    this.syncMode = 'pc';
    this._dragging = false;
    this._dragStart = 0;
    this._dragEnd = 0;
    this._boundDragMove = this._onDragMove.bind(this);
    this._boundDragUp = this._onDragUp.bind(this);
  }

  get _frames() {
    const b = this.frameBoundaries || [];
    if (b.length === 0) return [];
    const frames = [];
    for (let i = 0; i < b.length; i++) {
      const start = b[i];
      const end = i + 1 < b.length ? b[i + 1] : this.entryCount;
      frames.push({ index: i, start, end, size: end - start });
    }
    return frames;
  }

  /** Find which frame contains a given entry index */
  _frameAt(entryIdx) {
    const frames = this._frames;
    for (let i = frames.length - 1; i >= 0; i--) {
      if (entryIdx >= frames[i].start) return i;
    }
    return 0;
  }

  /** Is the current selection exactly "all"? */
  get _isAll() {
    return this.viewStart === 0 && this.viewEnd === this.entryCount;
  }

  /** Is the current selection exactly one frame? Returns frame index or -1 */
  get _currentFrame() {
    const frames = this._frames;
    for (let i = 0; i < frames.length; i++) {
      if (this.viewStart === frames[i].start && this.viewEnd === frames[i].end) return i;
    }
    return -1;
  }

  render() {
    const frames = this._frames;
    const hasFrames = frames.length > 1;
    const rangeSize = this.viewEnd - this.viewStart;
    const curFrame = this._currentFrame;

    return html`
      <div class="timeline">
        <div class="controls">
          <button class="${this._isAll ? 'active' : ''}"
            @click=${this._selectAll}>All</button>

          ${hasFrames ? html`
            <div class="frame-nav">
              <button @click=${() => this._stepFrame(-1)}
                ?disabled=${curFrame <= 0 && this._isAll}>&#9664;</button>
              <span class="frame-label">
                ${curFrame >= 0
                  ? html`Frame <strong>${curFrame + 1}</strong> / ${frames.length}`
                  : html`<strong>${frames.length}</strong> frames`
                }
              </span>
              <button @click=${() => this._stepFrame(1)}
                ?disabled=${curFrame >= frames.length - 1 && !this._isAll}>&#9654;</button>
            </div>
          ` : ''}

          ${this.compareMode ? html`
            <div class="sync-controls">
              <span class="sync-label">sync</span>
              ${[
                ['ly=0', 'frame'],
                ['pc', 'PC'],
                ['lcdc&80', 'LCD on'],
                ['none', 'none'],
              ].map(([mode, label]) => html`
                <button class="sync-btn ${this.syncMode === mode ? 'active' : ''}"
                  @click=${() => this._changeSync(mode)}
                >${label}</button>
              `)}
            </div>
          ` : ''}

          <span class="range-info">
            ${rangeSize.toLocaleString()} entries
            (${this.viewStart.toLocaleString()}..${this.viewEnd.toLocaleString()})
          </span>
        </div>

        <div class="bar-container"
          @mousedown=${this._onDragStart}
          @dblclick=${this._onDoubleClick}
        >
          ${hasFrames ? frames.map(f => {
            const left = (f.start / this.entryCount) * 100;
            return f.start > 0 ? html`
              <div class="frame-tick" style="left:${left}%"></div>
            ` : '';
          }) : ''}

          ${this.entryCount > 0 ? html`
            <div class="selection"
              style="left:${(this.viewStart / this.entryCount) * 100}%;
                     width:${(rangeSize / this.entryCount) * 100}%">
            </div>
            <div class="selection-edge-left"
              style="left:${(this.viewStart / this.entryCount) * 100}%">
            </div>
            <div class="selection-edge-right"
              style="left:${(this.viewEnd / this.entryCount) * 100}%">
            </div>
          ` : ''}
        </div>

        <div class="scrubber-row">
          <button class="scrub-step"
            @pointerdown=${() => this._startRepeat(-1)}
            @pointerup=${this._stopRepeat}
            @pointerleave=${this._stopRepeat}
          >&#9664;</button>
          <input type="range" class="scrubber"
            min=${this.viewStart} max=${Math.max(this.viewStart, this.viewEnd - 1)}
            .value=${String(this.currentIndex ?? this.viewStart)}
            @input=${this._onScrub}>
          <button class="scrub-step"
            @pointerdown=${() => this._startRepeat(1)}
            @pointerup=${this._stopRepeat}
            @pointerleave=${this._stopRepeat}
          >&#9654;</button>
        </div>
      </div>
    `;
  }

  // --- Selection ---

  _selectAll() {
    this._emitRange(0, this.entryCount);
  }

  _selectFrame(i) {
    const frames = this._frames;
    if (i < 0 || i >= frames.length) return;
    this._emitRange(frames[i].start, frames[i].end);
  }

  _stepFrame(delta) {
    const frames = this._frames;
    if (frames.length === 0) return;

    if (this._isAll) {
      // From "All", step to first or last frame
      this._selectFrame(delta > 0 ? 0 : frames.length - 1);
      return;
    }

    const cur = this._currentFrame;
    if (cur >= 0) {
      // Currently on a frame, step to adjacent
      const next = cur + delta;
      if (next >= 0 && next < frames.length) this._selectFrame(next);
    } else {
      // Arbitrary selection — snap to the nearest frame in the step direction
      const mid = (this.viewStart + this.viewEnd) / 2;
      const nearestFrame = this._frameAt(mid);
      const next = nearestFrame + delta;
      if (next >= 0 && next < frames.length) this._selectFrame(next);
      else this._selectFrame(nearestFrame);
    }
  }

  // --- Drag to select ---

  _posToEntry(e) {
    const bar = this.renderRoot.querySelector('.bar-container');
    if (!bar) return 0;
    const rect = bar.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    return Math.round(frac * this.entryCount);
  }

  _onDragStart(e) {
    e.preventDefault();
    this._dragging = true;
    this._dragStart = this._posToEntry(e);
    this._dragEnd = this._dragStart;
    window.addEventListener('mousemove', this._boundDragMove);
    window.addEventListener('mouseup', this._boundDragUp);
  }

  _onDragMove(e) {
    if (!this._dragging) return;
    this._dragEnd = this._posToEntry(e);
    const start = Math.min(this._dragStart, this._dragEnd);
    const end = Math.max(this._dragStart, this._dragEnd);
    // Live preview
    this._emitRange(start, Math.max(start + 1, end));
  }

  _onDragUp(e) {
    if (!this._dragging) return;
    this._dragging = false;
    window.removeEventListener('mousemove', this._boundDragMove);
    window.removeEventListener('mouseup', this._boundDragUp);

    this._dragEnd = this._posToEntry(e);
    let start = Math.min(this._dragStart, this._dragEnd);
    let end = Math.max(this._dragStart, this._dragEnd);

    // If it was basically a click (tiny drag), snap to the frame at that point
    if (end - start < this.entryCount * 0.005) {
      const frames = this._frames;
      if (frames.length > 1) {
        const frameIdx = this._frameAt(start);
        this._selectFrame(frameIdx);
        return;
      }
      // Single frame or no frames — select all
      this._selectAll();
      return;
    }

    this._emitRange(start, end);
  }

  _onDoubleClick() {
    this._selectAll();
  }

  _onScrub(e) {
    const index = parseInt(e.target.value, 10);
    this.dispatchEvent(new CustomEvent('current-index', {
      detail: { index },
      bubbles: true, composed: true,
    }));
  }

  _step(delta) {
    const cur = this.currentIndex ?? this.viewStart;
    const index = Math.max(this.viewStart, Math.min(this.viewEnd - 1, cur + delta));
    this.dispatchEvent(new CustomEvent('current-index', {
      detail: { index },
      bubbles: true, composed: true,
    }));
  }

  _startRepeat(delta) {
    this._step(delta);
    this._repeatDelta = delta;
    // Initial delay before repeat starts
    this._repeatTimeout = setTimeout(() => {
      this._repeatInterval = setInterval(() => this._step(this._repeatDelta), 50);
    }, 300);
  }

  _stopRepeat() {
    clearTimeout(this._repeatTimeout);
    clearInterval(this._repeatInterval);
    this._repeatTimeout = null;
    this._repeatInterval = null;
  }

  _changeSync(mode) {
    this.dispatchEvent(new CustomEvent('sync-changed', {
      detail: { sync: mode },
      bubbles: true, composed: true,
    }));
  }

  _emitRange(start, end) {
    start = Math.max(0, start);
    end = Math.min(this.entryCount, end);
    if (end <= start) end = Math.min(start + 1, this.entryCount);
    this.dispatchEvent(new CustomEvent('view-range-changed', {
      detail: { start, end },
      bubbles: true,
      composed: true,
    }));
  }

  /** Default view when a new trace is loaded. */
  updated(changed) {
    if (changed.has('frameBoundaries') || changed.has('entryCount')) {
      if (this.entryCount === 0) return;
      const frames = this._frames;
      if (frames.length > 3) {
        this._selectFrame(0);
      } else {
        this._selectAll();
      }
    }
  }
}

customElements.define('trace-timeline', TraceTimeline);
