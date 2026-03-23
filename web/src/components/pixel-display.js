import { LitElement, html, css } from 'lit';

const LCD_WIDTH = 160;
const LCD_HEIGHT = 144;
const SCALE = 2;

/**
 * Renders a Game Boy LCD frame from trace pixel data.
 *
 * Given a TraceStore with pixel data, renders the frame corresponding
 * to the current view position. Uses store.renderFrame() to get RGBA
 * pixel data and draws it to a scaled canvas.
 */
export class PixelDisplay extends LitElement {
  static properties = {
    store: { type: Object },
    frameBoundaries: { type: Array },
    viewStart: { type: Number },
    _frameIndex: { state: true },
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
    canvas {
      image-rendering: pixelated;
      border-radius: 4px;
    }
  `;

  constructor() {
    super();
    this.store = null;
    this.frameBoundaries = [];
    this.viewStart = 0;
    this._frameIndex = 0;
  }

  updated(changed) {
    if (changed.has('viewStart') || changed.has('frameBoundaries') || changed.has('store')) {
      this._updateFrame();
    }
  }

  /** Find which frame contains viewStart. */
  _currentFrame() {
    const bounds = this.frameBoundaries || [];
    if (bounds.length === 0) return 0;
    let frame = 0;
    for (let i = 0; i < bounds.length; i++) {
      if (bounds[i] <= this.viewStart) frame = i;
      else break;
    }
    return frame;
  }

  _updateFrame() {
    const fi = this._currentFrame();
    this._frameIndex = fi;
    if (!this.store) return;

    const canvas = this.renderRoot?.querySelector('canvas');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    try {
      const rgba = this.store.renderFrame(fi);
      if (!rgba) {
        ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT);
        return;
      }
      const imgData = new ImageData(new Uint8ClampedArray(rgba.buffer || rgba), LCD_WIDTH, LCD_HEIGHT);
      ctx.putImageData(imgData, 0, 0);
    } catch (err) {
      console.error('Failed to render frame:', err);
      ctx.clearRect(0, 0, LCD_WIDTH, LCD_HEIGHT);
    }
  }

  render() {
    const totalFrames = this.frameBoundaries?.length || 0;
    return html`
      <div class="pixel-wrap">
        <div class="pixel-header">
          <span class="pixel-title">pixels</span>
          <span class="frame-info">frame ${this._frameIndex + 1} / ${totalFrames}</span>
        </div>
        <canvas width=${LCD_WIDTH} height=${LCD_HEIGHT}
          style="width: ${LCD_WIDTH * SCALE}px; height: ${LCD_HEIGHT * SCALE}px;"
        ></canvas>
      </div>
    `;
  }
}

customElements.define('pixel-display', PixelDisplay);
