import { LitElement, html, css } from 'lit';

const SHADES = ['#e0f8d0', '#88c070', '#346856', '#081820'];
const PIXEL_SIZE = 16;
const FIFO_LEN = 8;

/**
 * Visualizes the PPU pixel pipeline as a left-to-right flow:
 *   [Tile Fetcher] → [FIFOs (BG + OBJ merge)] → [Output Pixel]
 *
 * Reads bgw_fifo_{a,b}, spr_fifo_{a,b}, mask_pipe, pal_pipe, bgp, obp0, obp1,
 * tfetch_state, sfetch_state, tile_temp_{a,b}, pix_count, sprite_count,
 * scan_count, rendering, win_mode, pix from the trace entry.
 */
export class PpuFifoVisualizer extends LitElement {
  static properties = {
    store: { type: Object },
    cursorIndex: { type: Number },
    _entry: { state: true },
  };

  static styles = css`
    :host { display: block; }
    .pipeline {
      display: flex;
      align-items: stretch;
      gap: 0;
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
      font-size: 0.7rem;
      font-family: var(--mono);
      overflow: hidden;
    }
    .stage {
      padding: 8px;
      display: flex;
      flex-direction: column;
      gap: 4px;
      min-width: 0;
    }
    .stage + .stage {
      border-left: 1px solid var(--border);
    }
    .stage-title {
      font-weight: 600;
      color: var(--accent);
      font-size: 0.65rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
      display: flex;
      align-items: center;
      gap: 4px;
    }
    .arrow {
      color: var(--text-muted);
      font-size: 0.9rem;
      display: flex;
      align-items: center;
      padding: 0 4px;
    }
    .fetcher-info {
      display: flex;
      flex-direction: column;
      gap: 3px;
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
    .fifo-section {
      display: flex;
      flex-direction: column;
      gap: 4px;
    }
    .fifo-row {
      display: flex;
      align-items: center;
      gap: 6px;
    }
    .fifo-label {
      width: 26px;
      color: var(--text-muted);
      flex-shrink: 0;
      font-size: 0.6rem;
    }
    canvas {
      image-rendering: pixelated;
      border: 1px solid var(--border);
      border-radius: 2px;
    }
    .merge-hint {
      font-size: 0.58rem;
      color: var(--text-muted);
      text-align: center;
      padding: 0 4px;
    }
    .output-section {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 4px;
      justify-content: center;
    }
    .output-pixel {
      width: 32px;
      height: 32px;
      border: 2px solid var(--border);
      border-radius: 4px;
    }
    .counter {
      display: flex;
      gap: 3px;
      align-items: baseline;
    }
    .counter-label { color: var(--text-muted); font-size: 0.6rem; }
    .counter-val { color: var(--text); font-weight: 600; }
    .counters {
      display: flex;
      gap: 8px;
      flex-wrap: wrap;
    }
    .flag-on { color: var(--accent); font-weight: 600; }
    .flag-off { color: var(--text-muted); opacity: 0.4; }
    .flags {
      display: flex;
      gap: 6px;
    }
    .pipe-info {
      font-size: 0.58rem;
      color: var(--text-muted);
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
      <div class="pipeline">
        <!-- Stage 1: Tile Fetcher -->
        <div class="stage">
          <div class="stage-title">Tile Fetch</div>
          <div class="fetcher-info">
            <div class="counter">
              <span class="counter-label">TFetch:</span>
              <span class="counter-val">${e.tfetch_state}</span>
            </div>
            <div class="counter">
              <span class="counter-label">SFetch:</span>
              <span class="counter-val">${e.sfetch_state}</span>
            </div>
            <div class="counter">
              <span class="counter-label">Tile row:</span>
            </div>
            ${this._renderTilePreview(e.tile_temp_a, e.tile_temp_b, e.bgp)}
          </div>
          <div class="flags">
            <span class="${e.rendering ? 'flag-on' : 'flag-off'}">REN</span>
            <span class="${e.win_mode ? 'flag-on' : 'flag-off'}">WIN</span>
          </div>
        </div>

        <div class="arrow">\u2192</div>

        <!-- Stage 2: FIFOs -->
        <div class="stage">
          <div class="stage-title">FIFOs</div>
          <div class="fifo-section">
            <div class="fifo-row">
              <span class="fifo-label">BG</span>
              <canvas id="bg-fifo" width="${FIFO_LEN * PIXEL_SIZE}" height="${PIXEL_SIZE}"></canvas>
            </div>
            <div class="merge-hint">priority / mask \u2193</div>
            <div class="fifo-row">
              <span class="fifo-label">OBJ</span>
              <canvas id="obj-fifo" width="${FIFO_LEN * PIXEL_SIZE}" height="${PIXEL_SIZE}"></canvas>
            </div>
          </div>
          <div class="counters">
            <span class="counter">
              <span class="counter-label">sprites:</span>
              <span class="counter-val">${e.sprite_count}</span>
            </span>
            <span class="counter">
              <span class="counter-label">scan:</span>
              <span class="counter-val">${e.scan_count}</span>
            </span>
          </div>
        </div>

        <div class="arrow">\u2192</div>

        <!-- Stage 3: Output Pixel -->
        <div class="stage">
          <div class="stage-title">Output</div>
          <div class="output-section">
            <div class="output-pixel" id="output-px"></div>
            <div class="counter">
              <span class="counter-label">pix:</span>
              <span class="counter-val">${e.pix_count}</span>
            </div>
            ${e.mask_pipe !== undefined ? html`
              <div class="pipe-info">mask: 0x${(e.mask_pipe ?? 0).toString(16).padStart(2, '0')}</div>
            ` : ''}
            ${e.pal_pipe !== undefined ? html`
              <div class="pipe-info">pal: 0x${(e.pal_pipe ?? 0).toString(16).padStart(2, '0')}</div>
            ` : ''}
          </div>
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
      this._drawOutputPixel(e);
    });
  }

  _drawFifo(canvasId, fifoA, fifoB, palette, mask) {
    const canvas = this.shadowRoot?.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');

    for (let i = 0; i < FIFO_LEN; i++) {
      const bitPos = i; // bit 0 on left (input), bit 7 on right (output)
      const lo = (fifoA >> bitPos) & 1;
      const hi = (fifoB >> bitPos) & 1;
      const colorIdx = (hi << 1) | lo;

      // Apply palette mapping
      const shade = (palette >> (colorIdx * 2)) & 3;

      // If mask is provided (OBJ FIFO), dim pixels where mask bit is 0
      const hasMask = mask !== undefined;
      const masked = hasMask && !((mask >> i) & 1);

      ctx.fillStyle = masked ? '#1a1a2e' : SHADES[shade];
      ctx.fillRect(i * PIXEL_SIZE, 0, PIXEL_SIZE, PIXEL_SIZE);

      // Draw grid lines
      ctx.strokeStyle = 'rgba(128,128,128,0.3)';
      ctx.strokeRect(i * PIXEL_SIZE, 0, PIXEL_SIZE, PIXEL_SIZE);
    }
  }

  _drawOutputPixel(e) {
    const el = this.shadowRoot?.getElementById('output-px');
    if (!el) return;

    // If we have a pix field, use it directly as a 2-bit shade index
    if (e.pix !== undefined) {
      const shade = e.pix & 3;
      el.style.background = SHADES[shade];
      return;
    }

    // Otherwise derive from the head of the BG FIFO after palette
    if (e.bgw_fifo_a !== undefined) {
      const lo = (e.bgw_fifo_a >> 7) & 1;
      const hi = (e.bgw_fifo_b >> 7) & 1;
      const colorIdx = (hi << 1) | lo;
      const shade = (e.bgp >> (colorIdx * 2)) & 3;
      el.style.background = SHADES[shade];
    } else {
      el.style.background = 'var(--bg)';
    }
  }

  _renderTilePreview(tileA, tileB, palette) {
    if (tileA === undefined) return html``;
    const pixels = [];
    for (let i = 0; i < 8; i++) {
      const bitPos = i; // match FIFO direction: bit 0 left, bit 7 right
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
