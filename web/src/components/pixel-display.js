import { LitElement, html, css } from 'lit';

const LCD_WIDTH = 160;
const LCD_HEIGHT = 144;
const SCALE = 2;

/**
 * Renders Game Boy LCD frames from trace pixel data.
 *
 * Single mode: one canvas showing the current frame, navigated by viewStart.
 * Compare mode (storeB set): three canvases — A | diff | B, aligned by
 * visual frame index so frame N in A is compared with frame N in B.
 */
export class PixelDisplay extends LitElement {
  static properties = {
    store: { type: Object },
    storeB: { type: Object },
    nameA: { type: String },
    nameB: { type: String },
    frameBoundaries: { type: Array },
    viewStart: { type: Number },
    _frameIndex: { state: true },
    _frameCountA: { state: true },
  };

  static styles = css`
    :host { display: block; }
    .pixel-wrap {
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
      padding: 8px;
    }
    .pixel-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: 4px;
      font-size: 0.75rem;
    }
    .pixel-title {
      font-family: var(--mono);
      font-weight: 600;
      color: var(--accent);
    }
    .frame-info {
      color: var(--text-muted);
    }
    .compare-row {
      display: flex;
      gap: 8px;
      align-items: flex-start;
    }
    .compare-panel {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 2px;
    }
    .compare-label {
      font-size: 0.7rem;
      font-family: var(--mono);
      color: var(--text-muted);
    }
    .compare-label.a { color: var(--accent); }
    .compare-label.b { color: #d29922; }
    .compare-label.diff { color: var(--red); }
    canvas {
      image-rendering: pixelated;
      border-radius: 4px;
    }
  `;

  constructor() {
    super();
    this.store = null;
    this.storeB = null;
    this.nameA = '';
    this.nameB = '';
    this.frameBoundaries = [];
    this.viewStart = 0;
    this._frameIndex = 0;
    this._frameCountA = 0;
  }

  updated(changed) {
    if (changed.has('store') || changed.has('storeB')) {
      this._frameCountA = this.store?.frameCount() || 0;
    }
    if (changed.has('viewStart') || changed.has('frameBoundaries') ||
        changed.has('store') || changed.has('storeB')) {
      this._syncFrameIndex();
      this._draw();
    }
  }

  /** In single mode, derive frame index from viewStart. In compare mode,
   *  also derive from viewStart (using trace A's boundaries) but apply
   *  the same index to both traces for visual alignment. */
  _syncFrameIndex() {
    const bounds = this.frameBoundaries || [];
    if (bounds.length === 0) {
      this._frameIndex = 0;
      return;
    }
    let frame = 0;
    for (let i = 0; i < bounds.length; i++) {
      if (bounds[i] <= this.viewStart) frame = i;
      else break;
    }
    this._frameIndex = frame;
  }

  _renderToCanvas(id, store, frameIndex) {
    const canvas = this.renderRoot?.querySelector(`#${id}`);
    if (!canvas) return null;
    const ctx = canvas.getContext('2d');
    if (!store) { ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT); return null; }
    try {
      const rgba = store.renderFrame(frameIndex);
      if (!rgba) { ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT); return null; }
      const arr = new Uint8ClampedArray(rgba.buffer || rgba);
      const imgData = new ImageData(arr, LCD_WIDTH, LCD_HEIGHT);
      ctx.putImageData(imgData, 0, 0);
      return arr;
    } catch (err) {
      console.error('Failed to render frame:', err);
      ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT);
      return null;
    }
  }

  _renderDiff(rgbaA, rgbaB) {
    const canvas = this.renderRoot?.querySelector('#diff');
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!rgbaA || !rgbaB) { ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT); return; }

    const diff = new Uint8ClampedArray(LCD_WIDTH * LCD_HEIGHT * 4);
    for (let i = 0; i < LCD_WIDTH * LCD_HEIGHT; i++) {
      const off = i * 4;
      const same = rgbaA[off] === rgbaB[off] &&
                   rgbaA[off+1] === rgbaB[off+1] &&
                   rgbaA[off+2] === rgbaB[off+2];
      if (same) {
        const shade = rgbaA[off];
        diff[off]   = shade;
        diff[off+1] = shade;
        diff[off+2] = shade;
      } else {
        const avg = (rgbaA[off] + rgbaB[off]) / 2;
        diff[off]   = Math.min(255, avg * 0.3 + 180);
        diff[off+1] = Math.round(avg * 0.2 + 30);
        diff[off+2] = Math.round(avg * 0.2 + 30);
      }
      diff[off+3] = 255;
    }
    const imgData = new ImageData(diff, LCD_WIDTH, LCD_HEIGHT);
    ctx.putImageData(imgData, 0, 0);
  }

  _draw() {
    const fi = this._frameIndex;
    if (this.storeB) {
      const rgbaA = this._renderToCanvas('canvasA', this.store, fi);
      const rgbaB = this._renderToCanvas('canvasB', this.storeB, fi);
      this._renderDiff(rgbaA, rgbaB);
    } else {
      this._renderToCanvas('canvasA', this.store, fi);
    }
  }

  render() {
    const total = this._frameCountA;

    if (this.storeB) {
      return html`
        <div class="pixel-wrap">
          <div class="pixel-header">
            <span class="pixel-title">pixels</span>
            <span class="frame-info">frame ${this._frameIndex + 1} / ${total}</span>
          </div>
          <div class="compare-row">
            <div class="compare-panel">
              <span class="compare-label a">${this.nameA || 'A'}</span>
              <canvas id="canvasA" width=${LCD_WIDTH} height=${LCD_HEIGHT}
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"
              ></canvas>
            </div>
            <div class="compare-panel">
              <span class="compare-label diff">diff</span>
              <canvas id="diff" width=${LCD_WIDTH} height=${LCD_HEIGHT}
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"
              ></canvas>
            </div>
            <div class="compare-panel">
              <span class="compare-label b">${this.nameB || 'B'}</span>
              <canvas id="canvasB" width=${LCD_WIDTH} height=${LCD_HEIGHT}
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"
              ></canvas>
            </div>
          </div>
        </div>
      `;
    }

    return html`
      <div class="pixel-wrap">
        <div class="pixel-header">
          <span class="pixel-title">pixels</span>
          <span class="frame-info">frame ${this._frameIndex + 1} / ${total}</span>
        </div>
        <canvas id="canvasA" width=${LCD_WIDTH} height=${LCD_HEIGHT}
          style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"
        ></canvas>
      </div>
    `;
  }
}

customElements.define('pixel-display', PixelDisplay);
