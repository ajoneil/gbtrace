import { LitElement, html, css } from 'lit';

/**
 * Timeline selector for navigating trace data by frame.
 *
 * Shows a horizontal bar with frame boundaries. The user can:
 * - Click a frame to select it
 * - Click+drag to select a range
 * - Use arrow keys or buttons to step through frames
 * - View the full trace by clicking "All"
 *
 * Emits `view-range-changed` with {start, end} entry indices.
 */
export class TraceTimeline extends LitElement {
  static properties = {
    /** Total entry count */
    entryCount: { type: Number },
    /** Frame boundary indices (Uint32Array from WASM) */
    frameBoundaries: { type: Array },
    /** Second trace frame boundaries (for compare mode) */
    frameBoundariesB: { type: Array },
    /** Current view start */
    viewStart: { type: Number },
    /** Current view end */
    viewEnd: { type: Number },
    /** Compare mode */
    compareMode: { type: Boolean },
    /** Entry count of trace B */
    entryCountB: { type: Number },
    /** Internal: which frame is selected (-1 = all) */
    _selectedFrame: { state: true },
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
    }

    .frame-nav {
      display: flex;
      align-items: center;
      gap: 4px;
    }

    .frame-nav button, .all-btn {
      padding: 2px 8px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      color: var(--text-muted);
      cursor: pointer;
      font-family: var(--mono);
      font-size: 0.75rem;
    }
    .frame-nav button:hover, .all-btn:hover {
      border-color: var(--accent);
      color: var(--accent);
    }
    .all-btn.active {
      background: var(--accent-subtle);
      border-color: var(--accent);
      color: var(--accent);
    }

    .frame-label {
      color: var(--text-muted);
      font-family: var(--mono);
      font-size: 0.75rem;
      min-width: 120px;
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

    /* Visual timeline bar */
    .bar-container {
      position: relative;
      height: 24px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 4px;
      overflow: hidden;
      cursor: pointer;
    }

    .frame-segment {
      position: absolute;
      top: 0;
      height: 100%;
      border-right: 1px solid var(--border);
      transition: background 0.1s;
    }
    .frame-segment:hover {
      background: var(--bg-hover);
    }
    .frame-segment.selected {
      background: var(--accent-subtle);
    }
    .frame-segment:last-child {
      border-right: none;
    }

    /* Selection highlight overlay */
    .selection {
      position: absolute;
      top: 0;
      height: 100%;
      background: var(--accent-subtle);
      border-left: 2px solid var(--accent);
      border-right: 2px solid var(--accent);
      pointer-events: none;
    }
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
    this._selectedFrame = -1; // -1 = show all
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

  get _totalFrames() {
    return this._frames.length;
  }

  render() {
    const frames = this._frames;
    const hasFrames = frames.length > 1;
    const showAll = this._selectedFrame === -1;
    const rangeSize = this.viewEnd - this.viewStart;

    return html`
      <div class="timeline">
        <div class="controls">
          ${hasFrames ? html`
            <button class="all-btn ${showAll ? 'active' : ''}"
              @click=${() => this._selectAll()}>All</button>

            <div class="frame-nav">
              <button @click=${() => this._stepFrame(-1)}
                ?disabled=${showAll || this._selectedFrame <= 0}>◀</button>
              <span class="frame-label">
                ${showAll
                  ? html`<strong>${frames.length}</strong> frames`
                  : html`Frame <strong>${this._selectedFrame + 1}</strong> / ${frames.length}`
                }
              </span>
              <button @click=${() => this._stepFrame(1)}
                ?disabled=${showAll || this._selectedFrame >= frames.length - 1}>▶</button>
            </div>
          ` : html`
            <span class="frame-label"><strong>1</strong> frame</span>
          `}

          <span class="range-info">
            ${rangeSize.toLocaleString()} entries
            (${this.viewStart.toLocaleString()}–${this.viewEnd.toLocaleString()})
          </span>
        </div>

        ${hasFrames ? html`
          <div class="bar-container">
            ${frames.map((f, i) => {
              const left = (f.start / this.entryCount) * 100;
              const width = (f.size / this.entryCount) * 100;
              const selected = !showAll && i === this._selectedFrame;
              return html`
                <div class="frame-segment ${selected ? 'selected' : ''}"
                  style="left:${left}%;width:${width}%"
                  @click=${() => this._selectFrame(i)}
                  title="Frame ${i + 1}: ${f.size.toLocaleString()} entries"
                ></div>
              `;
            })}
            ${!showAll ? html`
              <div class="selection"
                style="left:${(this.viewStart / this.entryCount) * 100}%;
                       width:${(rangeSize / this.entryCount) * 100}%">
              </div>
            ` : ''}
          </div>
        ` : ''}
      </div>
    `;
  }

  _selectAll() {
    this._selectedFrame = -1;
    this._emitRange(0, this.entryCount);
  }

  _selectFrame(i) {
    const frames = this._frames;
    if (i < 0 || i >= frames.length) return;
    this._selectedFrame = i;
    this._emitRange(frames[i].start, frames[i].end);
  }

  _stepFrame(delta) {
    const next = this._selectedFrame + delta;
    this._selectFrame(next);
  }

  _emitRange(start, end) {
    this.dispatchEvent(new CustomEvent('view-range-changed', {
      detail: { start, end },
      bubbles: true,
      composed: true,
    }));
  }

  /** When a new trace is loaded, default to showing all or first frame for large traces. */
  updated(changed) {
    if (changed.has('frameBoundaries') || changed.has('entryCount')) {
      const frames = this._frames;
      if (frames.length > 3) {
        // Default to first frame for multi-frame traces
        this._selectFrame(0);
      } else {
        this._selectAll();
      }
    }
  }
}

customElements.define('trace-timeline', TraceTimeline);
