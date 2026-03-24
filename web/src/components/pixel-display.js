import { LitElement, html, css } from 'lit';

const LCD_WIDTH = 160;
const LCD_HEIGHT = 144;
const SCALE = 2;

/**
 * Renders Game Boy LCD frames from trace pixel data.
 *
 * Single mode: one canvas showing the current frame.
 * Compare mode (storeB set): three canvases — A | diff | B.
 * T-cycle mode (tcyclePixels): renders partial frames at the given
 * currentIndex and supports pixel crosshair highlighting.
 */
export class PixelDisplay extends LitElement {
  static properties = {
    store: { type: Object },
    storeB: { type: Object },
    nameA: { type: String },
    nameB: { type: String },
    frameBoundaries: { type: Array },
    viewStart: { type: Number },
    tcyclePixels: { type: Boolean },
    currentIndex: { type: Number },
    _frameIndex: { state: true },
    _frameCountA: { state: true },
    _highlightPixel: { state: true },
    _pixMap: { state: true },
    _pixMapFrame: { state: true },
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
    .canvas-wrap {
      position: relative;
      display: block;
      width: fit-content;
    }
    .highlight-overlay {
      position: absolute;
      top: 0;
      left: 0;
      pointer-events: none;
      image-rendering: pixelated;
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
    this.tcyclePixels = false;
    this.currentIndex = null;
    this._frameIndex = 0;
    this._frameCountA = 0;
    this._highlightPixel = null;
    this._pixMap = null;
    this._pixMapFrame = -1;
  }

  updated(changed) {
    if (changed.has('store') || changed.has('storeB')) {
      this._frameCountA = this.store?.frameCount() || 0;
      this._pixMap = null;
      this._pixMapFrame = -1;
    }
    if (changed.has('viewStart') || changed.has('frameBoundaries') ||
        changed.has('store') || changed.has('storeB')) {
      this._syncFrameIndex();
      this._draw();
    }
    if (changed.has('currentIndex') && this.tcyclePixels && this.currentIndex != null) {
      this._updateForCurrentIndex();
    }
  }

  _syncFrameIndex() {
    const bounds = this.frameBoundaries || [];
    let frame = 0;
    for (let i = 0; i < bounds.length; i++) {
      if (bounds[i] <= this.viewStart) frame = i;
      else break;
    }
    if (frame !== this._frameIndex) {
      this._frameIndex = frame;
      this._pixMap = null;
      this._pixMapFrame = -1;
    }
  }

  _getFrameRange() {
    const bounds = this.frameBoundaries || [];
    const fi = this._frameIndex;
    const start = bounds[fi] || 0;
    const end = fi + 1 < bounds.length ? bounds[fi + 1] : (this.store?.entryCount() || 0);
    return { start, end };
  }

  _ensurePixMap() {
    if (this._pixMapFrame === this._frameIndex || !this.store || !this.tcyclePixels) return;
    try {
      this._pixMap = this.store.buildPixelPositionMap(this._frameIndex);
      this._pixMapFrame = this._frameIndex;
    } catch (_) {
      this._pixMap = null;
    }
  }

  _updateForCurrentIndex() {
    if (!this.tcyclePixels || this.currentIndex == null) {
      if (this._highlightPixel) {
        this._highlightPixel = null;
        this._drawHighlight();
      }
      this._draw();
      return;
    }
    this._ensurePixMap();

    const { start } = this._getFrameRange();

    // Update pixel highlight from position map
    if (this._pixMap) {
      const mapIdx = this.currentIndex - start;
      if (mapIdx < 0 || mapIdx >= this._pixMap.length) {
        this._highlightPixel = null;
      } else {
        const packed = this._pixMap[mapIdx];
        if (packed === 0xFFFFFFFF) {
          this._highlightPixel = null;
        } else {
          this._highlightPixel = { x: packed >> 16, y: packed & 0xFFFF };
        }
      }
      this._drawHighlight();
    }

    // Draw partial frame at current index
    if (this.currentIndex >= start) {
      this._drawPartialAt(this.currentIndex);
    }
  }

  /** Draw a partial frame at a specific entry index. */
  _drawPartialAt(entry) {
    if (!this.store || !this.tcyclePixels) return;
    const canvas = this.renderRoot?.querySelector('#canvasA');
    if (!canvas) return;
    try {
      const ctx = canvas.getContext('2d');
      if (!this._tmpCanvas) {
        this._tmpCanvas = document.createElement('canvas');
        this._tmpCanvas.width = LCD_WIDTH;
        this._tmpCanvas.height = LCD_HEIGHT;
      }
      const tmp = this._tmpCanvas.getContext('2d');

      // Draw the full completed frame as a faded background
      const fullRgba = this.store.renderFrame(this._frameIndex);
      if (fullRgba) {
        const fullArr = new Uint8ClampedArray(fullRgba.buffer || fullRgba);
        tmp.putImageData(new ImageData(fullArr, LCD_WIDTH, LCD_HEIGHT), 0, 0);
        this._drawCheckerboard(ctx);
        ctx.globalAlpha = 0.25;
        ctx.drawImage(this._tmpCanvas, 0, 0);
        ctx.globalAlpha = 1.0;
      } else {
        this._drawCheckerboard(ctx);
      }

      // Draw the partial frame (up to current entry) at full opacity on top
      const rgba = this.store.renderPartialFrame(this._frameIndex, entry);
      if (!rgba) return;
      const arr = new Uint8ClampedArray(rgba.buffer || rgba);
      tmp.putImageData(new ImageData(arr, LCD_WIDTH, LCD_HEIGHT), 0, 0);
      ctx.drawImage(this._tmpCanvas, 0, 0);
    } catch (err) {
      console.error('Failed to render partial frame:', err);
    }
  }

  _drawHighlight() {
    const overlay = this.renderRoot?.querySelector('.highlight-overlay');
    if (!overlay) return;
    const ctx = overlay.getContext('2d');
    ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT);
    if (!this._highlightPixel) return;
    const { x, y } = this._highlightPixel;
    ctx.strokeStyle = '#ff4444';
    ctx.lineWidth = 1;
    // Draw crosshair
    ctx.beginPath();
    ctx.moveTo(x, 0); ctx.lineTo(x, LCD_HEIGHT);
    ctx.moveTo(0, y + 0.5); ctx.lineTo(LCD_WIDTH, y + 0.5);
    ctx.stroke();
    // Draw pixel highlight
    ctx.fillStyle = 'rgba(255,68,68,0.5)';
    ctx.fillRect(x, y, 1, 1);
  }

  _drawCheckerboard(ctx) {
    const size = 4; // checker size in LCD pixels
    for (let y = 0; y < LCD_HEIGHT; y += size) {
      for (let x = 0; x < LCD_WIDTH; x += size) {
        const dark = ((x / size) + (y / size)) % 2 === 0;
        ctx.fillStyle = dark ? '#1a1a2e' : '#16213e';
        ctx.fillRect(x, y, size, size);
      }
    }
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
      // Draw checkerboard for unrendered areas, then composite frame over it
      this._drawCheckerboard(ctx);
      if (!this._tmpCanvas) {
        this._tmpCanvas = document.createElement('canvas');
        this._tmpCanvas.width = LCD_WIDTH;
        this._tmpCanvas.height = LCD_HEIGHT;
      }
      const tmp = this._tmpCanvas.getContext('2d');
      tmp.putImageData(new ImageData(arr, LCD_WIDTH, LCD_HEIGHT), 0, 0);
      ctx.drawImage(this._tmpCanvas, 0, 0);
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
      const same = rgbaA[off] === rgbaB[off] && rgbaA[off+1] === rgbaB[off+1] && rgbaA[off+2] === rgbaB[off+2];
      if (same) {
        diff[off] = rgbaA[off]; diff[off+1] = rgbaA[off+1]; diff[off+2] = rgbaA[off+2];
      } else {
        const avg = (rgbaA[off] + rgbaB[off]) / 2;
        diff[off] = Math.min(255, avg * 0.3 + 180);
        diff[off+1] = Math.round(avg * 0.2 + 30);
        diff[off+2] = Math.round(avg * 0.2 + 30);
      }
      diff[off+3] = 255;
    }
    ctx.putImageData(new ImageData(diff, LCD_WIDTH, LCD_HEIGHT), 0, 0);
  }

  _draw() {
    const fi = this._frameIndex;
    if (this.storeB) {
      const rgbaA = this._renderToCanvas('canvasA', this.store, fi);
      const rgbaB = this._renderToCanvas('canvasB', this.storeB, fi);
      this._renderDiff(rgbaA, rgbaB);
    } else if (this.currentIndex != null && this.tcyclePixels) {
      this._drawPartialAt(this.currentIndex);
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
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"></canvas>
            </div>
            <div class="compare-panel">
              <span class="compare-label diff">diff</span>
              <canvas id="diff" width=${LCD_WIDTH} height=${LCD_HEIGHT}
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"></canvas>
            </div>
            <div class="compare-panel">
              <span class="compare-label b">${this.nameB || 'B'}</span>
              <canvas id="canvasB" width=${LCD_WIDTH} height=${LCD_HEIGHT}
                style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"></canvas>
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
        <div class="canvas-wrap">
          <canvas id="canvasA" width=${LCD_WIDTH} height=${LCD_HEIGHT}
            style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"></canvas>
          ${this.tcyclePixels ? html`
            <canvas class="highlight-overlay" width=${LCD_WIDTH} height=${LCD_HEIGHT}
              style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"></canvas>
          ` : ''}
        </div>
      </div>
    `;
  }
}

customElements.define('pixel-display', PixelDisplay);
