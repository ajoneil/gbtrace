import { LitElement, html, css } from 'lit';

const SHADES = ['#e0f0e0', '#88c070', '#305830', '#0f1f0f'];
const PIXEL_SIZE = 16;
const FIFO_LEN = 8;

/**
 * Visualizes PPU pixel FIFO contents, fetcher state, and pipeline counters.
 * Reads bgw_fifo_{a,b}, spr_fifo_{a,b}, mask_pipe, pal_pipe, bgp, obp0, obp1,
 * tfetch_state, sfetch_state, tile_temp_{a,b}, pix_count, sprite_count,
 * scan_count, rendering, win_mode from the trace entry.
 */
export class PpuFifoVisualizer extends LitElement {
  static properties = {
    store: { type: Object },
    cursorIndex: { type: Number },
    _entry: { state: true },
  };

  static styles = css`
    :host { display: block; }
    .wrap {
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
      padding: 8px;
      font-size: 0.7rem;
      font-family: var(--mono);
    }
    .title {
      font-weight: 600;
      color: var(--accent);
      margin-bottom: 4px;
    }
    .fifo-row {
      display: flex;
      align-items: center;
      gap: 6px;
      margin-bottom: 4px;
    }
    .fifo-label {
      width: 28px;
      color: var(--text-muted);
      flex-shrink: 0;
      font-size: 0.65rem;
    }
    canvas {
      image-rendering: pixelated;
      border: 1px solid var(--border);
      border-radius: 2px;
    }
    .counters {
      display: flex;
      gap: 10px;
      margin-top: 6px;
      flex-wrap: wrap;
    }
    .counter {
      display: flex;
      gap: 3px;
      align-items: baseline;
    }
    .counter-label { color: var(--text-muted); }
    .counter-val { color: var(--text); font-weight: 600; }
    .flag-on { color: var(--accent); font-weight: 600; }
    .flag-off { color: var(--text-muted); opacity: 0.4; }
    .fetcher-row {
      display: flex;
      gap: 10px;
      margin-top: 4px;
      align-items: center;
    }
    .tile-preview {
      display: flex;
      gap: 0;
    }
    .tile-px {
      width: 8px;
      height: 8px;
      border: 0.5px solid var(--border);
    }
  `;

  constructor() {
    super();
    this._entry = null;
    this._pendingUpdate = false;
  }

  updated(changed) {
    if ((changed.has('cursorIndex') || changed.has('store')) && this.store && this.cursorIndex >= 0) {
      if (!this._pendingUpdate) {
        this._pendingUpdate = true;
        requestAnimationFrame(() => {
          this._pendingUpdate = false;
          this._entry = this.store.entry(this.cursorIndex);
          this._drawFifos();
        });
      }
    }
  }

  render() {
    if (!this._entry || this._entry.bgw_fifo_a === undefined) return html``;
    const e = this._entry;

    return html`
      <div class="wrap">
        <div class="title">Pixel Pipeline</div>

        <div class="fifo-row">
          <span class="fifo-label">BG</span>
          <canvas id="bg-fifo" width="${FIFO_LEN * PIXEL_SIZE}" height="${PIXEL_SIZE}"></canvas>
          <span class="fifo-label">OBJ</span>
          <canvas id="obj-fifo" width="${FIFO_LEN * PIXEL_SIZE}" height="${PIXEL_SIZE}"></canvas>
        </div>

        <div class="fetcher-row">
          <span class="counter">
            <span class="counter-label">TFetch:</span>
            <span class="counter-val">${e.tfetch_state}</span>
          </span>
          <span class="counter">
            <span class="counter-label">SFetch:</span>
            <span class="counter-val">${e.sfetch_state}</span>
          </span>
          <span class="counter">
            <span class="counter-label">Tile:</span>
          </span>
          ${this._renderTilePreview(e.tile_temp_a, e.tile_temp_b, e.bgp)}
        </div>

        <div class="counters">
          <span class="counter">
            <span class="counter-label">pix:</span>
            <span class="counter-val">${e.pix_count}</span>
          </span>
          <span class="counter">
            <span class="counter-label">sprites:</span>
            <span class="counter-val">${e.sprite_count}</span>
          </span>
          <span class="counter">
            <span class="counter-label">scan:</span>
            <span class="counter-val">${e.scan_count}</span>
          </span>
          <span class="${e.rendering ? 'flag-on' : 'flag-off'}">RENDER</span>
          <span class="${e.win_mode ? 'flag-on' : 'flag-off'}">WIN</span>
        </div>
      </div>
    `;
  }

  _drawFifos() {
    if (!this._entry) return;
    const e = this._entry;

    this.updateComplete.then(() => {
      this._drawFifo('bg-fifo', e.bgw_fifo_a, e.bgw_fifo_b, e.bgp);
      this._drawFifo('obj-fifo', e.spr_fifo_a, e.spr_fifo_b, e.obp0, e.mask_pipe);
    });
  }

  _drawFifo(canvasId, fifoA, fifoB, palette, mask) {
    const canvas = this.shadowRoot?.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');

    for (let i = 0; i < FIFO_LEN; i++) {
      const bitPos = 7 - i; // bit 7 = leftmost (next to shift out)
      const lo = (fifoA >> bitPos) & 1;
      const hi = (fifoB >> bitPos) & 1;
      const colorIdx = (hi << 1) | lo;

      // Apply palette mapping
      const shade = (palette >> (colorIdx * 2)) & 3;

      // If mask is provided (OBJ FIFO), dim pixels where mask bit is 0
      const hasMask = mask !== undefined;
      const masked = hasMask && !((mask >> bitPos) & 1);

      ctx.fillStyle = masked ? '#1a1a2e' : SHADES[shade];
      ctx.fillRect(i * PIXEL_SIZE, 0, PIXEL_SIZE, PIXEL_SIZE);

      // Draw grid lines
      ctx.strokeStyle = 'rgba(128,128,128,0.3)';
      ctx.strokeRect(i * PIXEL_SIZE, 0, PIXEL_SIZE, PIXEL_SIZE);
    }
  }

  _renderTilePreview(tileA, tileB, palette) {
    if (tileA === undefined) return html``;
    const pixels = [];
    for (let i = 0; i < 8; i++) {
      const bitPos = 7 - i;
      const lo = (tileA >> bitPos) & 1;
      const hi = (tileB >> bitPos) & 1;
      const colorIdx = (hi << 1) | lo;
      const shade = (palette >> (colorIdx * 2)) & 3;
      pixels.push(html`<div class="tile-px" style="background:${SHADES[shade]}"></div>`);
    }
    return html`<div class="tile-preview">${pixels}</div>`;
  }
}

customElements.define('ppu-fifo-visualizer', PpuFifoVisualizer);
