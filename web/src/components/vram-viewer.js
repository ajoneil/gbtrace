import { LitElement, html, css } from 'lit';

const TILE_SHEET_WIDTH = 128;
const TILE_SHEET_HEIGHT = 192;
const TILEMAP_SIZE = 256;
const SCALE = 2;

/**
 * VRAM tile and tilemap viewer.
 *
 * Shows the 384-tile sheet and BG/window tilemaps reconstructed
 * from vram_addr/vram_data trace fields. Updates as the current
 * trace entry changes.
 */
export class VramViewer extends LitElement {
  static properties = {
    store: { type: Object },
    currentIndex: { type: Number, attribute: 'current-index' },
    _tab: { state: true },
  };

  static styles = css`
    :host { display: block; }
    .vram-wrap {
      display: flex;
      flex-direction: column;
      gap: 4px;
    }
    .tabs {
      display: flex;
      gap: 4px;
    }
    .tab {
      padding: 3px 8px;
      background: var(--bg-secondary, #1a1a2e);
      border: 1px solid var(--border, #333);
      border-radius: 3px;
      color: var(--text-secondary, #888);
      cursor: pointer;
      font: inherit;
      font-size: 0.75rem;
    }
    .tab:hover { color: var(--text, #eee); border-color: var(--accent, #4fc3f7); }
    .tab.active {
      color: var(--accent, #4fc3f7);
      border-color: var(--accent, #4fc3f7);
    }
    .canvas-wrap {
      background: var(--bg-secondary, #1a1a2e);
      border: 1px solid var(--border, #333);
      border-radius: 3px;
      padding: 4px;
      display: inline-block;
    }
    canvas {
      image-rendering: pixelated;
    }
    .label {
      font-size: 0.7rem;
      color: var(--text-secondary, #888);
      margin-top: 2px;
    }
  `;

  constructor() {
    super();
    this._tab = 'tiles';
    this._lastRenderedEntry = -1;
    this._lastRenderedTab = null;
  }

  updated(changed) {
    if (!this.store?.hasVramData?.()) return;

    const needsRedraw =
      changed.has('store') ||
      changed.has('_tab') ||
      (changed.has('currentIndex') && this.currentIndex !== this._lastRenderedEntry);

    if (needsRedraw) {
      this._draw();
    }
  }

  _draw() {
    if (!this.store || this.currentIndex == null) return;

    const entry = this.currentIndex;
    const tab = this._tab;

    if (tab === 'tiles') {
      this._drawTileSheet(entry);
    } else {
      this._drawTilemap(entry, tab === 'window' ? 1 : 0);
    }

    this._lastRenderedEntry = entry;
    this._lastRenderedTab = tab;
  }

  _drawTileSheet(entry) {
    const canvas = this.renderRoot?.querySelector('canvas');
    if (!canvas) return;
    canvas.width = TILE_SHEET_WIDTH;
    canvas.height = TILE_SHEET_HEIGHT;
    canvas.style.width = `${TILE_SHEET_WIDTH * SCALE}px`;
    canvas.style.height = `${TILE_SHEET_HEIGHT * SCALE}px`;

    try {
      const rgba = this.store.renderTileSheet(entry);
      if (!rgba) return;
      const ctx = canvas.getContext('2d');
      const arr = new Uint8ClampedArray(rgba.buffer || rgba);
      ctx.putImageData(new ImageData(arr, TILE_SHEET_WIDTH, TILE_SHEET_HEIGHT), 0, 0);
    } catch (err) {
      console.error('Failed to render tile sheet:', err);
    }
  }

  _drawTilemap(entry, mapSelect) {
    const canvas = this.renderRoot?.querySelector('canvas');
    if (!canvas) return;
    canvas.width = TILEMAP_SIZE;
    canvas.height = TILEMAP_SIZE;
    canvas.style.width = `${TILEMAP_SIZE * SCALE}px`;
    canvas.style.height = `${TILEMAP_SIZE * SCALE}px`;

    try {
      const rgba = this.store.renderTilemap(entry, mapSelect);
      if (!rgba) return;
      const ctx = canvas.getContext('2d');
      const arr = new Uint8ClampedArray(rgba.buffer || rgba);
      ctx.putImageData(new ImageData(arr, TILEMAP_SIZE, TILEMAP_SIZE), 0, 0);
    } catch (err) {
      console.error('Failed to render tilemap:', err);
    }
  }

  render() {
    if (!this.store?.hasVramData?.()) return html``;

    return html`
      <div class="vram-wrap">
        <div class="tabs">
          <button class="tab ${this._tab === 'tiles' ? 'active' : ''}"
                  @click=${() => this._tab = 'tiles'}>Tiles</button>
          <button class="tab ${this._tab === 'bg' ? 'active' : ''}"
                  @click=${() => this._tab = 'bg'}>BG Map</button>
          <button class="tab ${this._tab === 'window' ? 'active' : ''}"
                  @click=${() => this._tab = 'window'}>Window Map</button>
        </div>
        <div class="canvas-wrap">
          <canvas></canvas>
        </div>
        <div class="label">
          ${this._tab === 'tiles' ? '384 tiles (16×24 grid)' :
            this._tab === 'bg' ? 'Background tilemap (32×32)' :
            'Window tilemap (32×32)'}
          ${this.currentIndex != null ? ` @ entry ${this.currentIndex}` : ''}
        </div>
      </div>
    `;
  }
}

customElements.define('vram-viewer', VramViewer);
