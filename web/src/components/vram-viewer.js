import { LitElement, html, css } from 'lit';

const TILE_SHEET_WIDTH = 128;
const TILE_SHEET_HEIGHT = 192;
const TILEMAP_SIZE = 256;
const SCALE = 2;
const LCD_WIDTH = 160;
const LCD_HEIGHT = 144;

/**
 * VRAM tile and tilemap viewer.
 *
 * Shows the 384-tile sheet and BG/window tilemaps reconstructed
 * from vram_addr/vram_data trace fields. Updates as the current
 * trace entry changes.
 *
 * Features:
 * - Viewport rectangle on tilemaps showing the visible screen area
 * - Sprite tile highlight on hover (from sprite table)
 */
export class VramViewer extends LitElement {
  static properties = {
    store: { type: Object },
    currentIndex: { type: Number, attribute: 'current-index' },
    hoveredSprite: { type: Number, attribute: 'hovered-sprite' },
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
      position: relative;
    }
    canvas {
      image-rendering: pixelated;
      display: block;
    }
    .overlay {
      position: absolute;
      top: 4px;
      left: 4px;
      pointer-events: none;
    }
    .label {
      font-size: 0.7rem;
      color: var(--text-secondary, #888);
      margin-top: 2px;
    }
    .sprite-preview {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 3px 6px;
      background: var(--bg-secondary, #1a1a2e);
      border: 1px solid var(--accent, #4fc3f7);
      border-radius: 3px;
      font-size: 0.7rem;
      color: var(--text-secondary, #888);
    }
    .sprite-preview canvas {
      border: 1px solid var(--border, #333);
    }
  `;

  constructor() {
    super();
    this._tab = 'tiles';
    this._lastRenderedEntry = -1;
    this._lastRenderedTab = null;
    this._lastHoveredSprite = -1;
    this._drawPending = false;
  }

  updated(changed) {
    if (!this.store?.hasVramData?.()) return;

    const needsRedraw =
      changed.has('store') ||
      changed.has('_tab') ||
      changed.has('hoveredSprite') ||
      (changed.has('currentIndex') && this.currentIndex !== this._lastRenderedEntry);

    if (needsRedraw) {
      // Throttle redraws to avoid WASM calls on every hover event
      if (!this._drawPending) {
        this._drawPending = true;
        requestAnimationFrame(() => {
          this._drawPending = false;
          this._draw();
        });
      }
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

    this._drawOverlay(entry, tab);
    this._drawSpritePreview(entry);

    this._lastRenderedEntry = entry;
    this._lastRenderedTab = tab;
    this._lastHoveredSprite = this.hoveredSprite;
  }

  _drawTileSheet(entry) {
    const canvas = this.renderRoot?.querySelector('#vram-canvas');
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
    const canvas = this.renderRoot?.querySelector('#vram-canvas');
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

  _drawOverlay(entry, tab) {
    const overlay = this.renderRoot?.querySelector('.overlay');
    if (!overlay) return;

    const e = this.store.entry(entry);
    if (!e) return;

    if (tab === 'tiles') {
      // On tile sheet: highlight hovered sprite's tile
      overlay.width = TILE_SHEET_WIDTH;
      overlay.height = TILE_SHEET_HEIGHT;
      overlay.style.width = `${TILE_SHEET_WIDTH * SCALE}px`;
      overlay.style.height = `${TILE_SHEET_HEIGHT * SCALE}px`;
      const ctx = overlay.getContext('2d');
      ctx.clearRect(0, 0, TILE_SHEET_WIDTH, TILE_SHEET_HEIGHT);

      if (this.hoveredSprite != null && this.hoveredSprite >= 0) {
        const tileId = e[`oam${this.hoveredSprite}_id`];
        if (tileId !== undefined) {
          const lcdc = e.lcdc || 0;
          const tallSprites = (lcdc & 0x04) !== 0; // bit 2: 8x16 mode
          const tileX = (tileId % 16) * 8;
          const tileY = Math.floor(tileId / 16) * 8;

          ctx.strokeStyle = '#ff4444';
          ctx.lineWidth = 1;
          ctx.strokeRect(tileX + 0.5, tileY + 0.5, 7, tallSprites ? 15 : 7);
        }
      }
    } else {
      // On tilemap: draw viewport rectangle
      overlay.width = TILEMAP_SIZE;
      overlay.height = TILEMAP_SIZE;
      overlay.style.width = `${TILEMAP_SIZE * SCALE}px`;
      overlay.style.height = `${TILEMAP_SIZE * SCALE}px`;
      const ctx = overlay.getContext('2d');
      ctx.clearRect(0, 0, TILEMAP_SIZE, TILEMAP_SIZE);

      if (tab === 'bg') {
        const scx = e.scx ?? 0;
        const scy = e.scy ?? 0;
        this._drawViewport(ctx, scx, scy, '#4fc3f7');
      } else {
        // Window viewport: WX-7, WY
        const wx = (e.wx ?? 7) - 7;
        const wy = e.wy ?? 0;
        // Window shows from (0,0) in the tilemap, viewport at (wx,wy) on screen
        ctx.strokeStyle = '#ffb74d';
        ctx.lineWidth = 1;
        ctx.setLineDash([3, 3]);
        ctx.strokeRect(0.5, 0.5, LCD_WIDTH - wx - 1, LCD_HEIGHT - wy - 1);
        ctx.setLineDash([]);
      }
    }
  }

  /** Draw a 160x144 viewport rect that wraps around the 256x256 tilemap. */
  _drawViewport(ctx, scrollX, scrollY, color) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.setLineDash([3, 3]);

    // The viewport wraps around the tilemap edges
    const x = scrollX;
    const y = scrollY;
    const w = LCD_WIDTH;
    const h = LCD_HEIGHT;

    // Draw up to 4 rectangles for wrap-around
    const rects = [];
    if (x + w <= 256 && y + h <= 256) {
      // No wrapping
      rects.push([x, y, w, h]);
    } else if (x + w > 256 && y + h <= 256) {
      // Wraps horizontally
      rects.push([x, y, 256 - x, h]);
      rects.push([0, y, w - (256 - x), h]);
    } else if (x + w <= 256 && y + h > 256) {
      // Wraps vertically
      rects.push([x, y, w, 256 - y]);
      rects.push([x, 0, w, h - (256 - y)]);
    } else {
      // Wraps both
      rects.push([x, y, 256 - x, 256 - y]);
      rects.push([0, y, w - (256 - x), 256 - y]);
      rects.push([x, 0, 256 - x, h - (256 - y)]);
      rects.push([0, 0, w - (256 - x), h - (256 - y)]);
    }

    for (const [rx, ry, rw, rh] of rects) {
      ctx.strokeRect(rx + 0.5, ry + 0.5, rw - 1, rh - 1);
    }
    ctx.setLineDash([]);
  }

  _drawSpritePreview(entry) {
    const canvas = this.renderRoot?.querySelector('#sprite-preview-canvas');
    if (!canvas) return;

    if (this.hoveredSprite == null || this.hoveredSprite < 0) {
      canvas.style.display = 'none';
      return;
    }

    const e = this.store.entry(entry);
    if (!e) return;

    const tileId = e[`oam${this.hoveredSprite}_id`];
    const attr = e[`oam${this.hoveredSprite}_attr`];
    if (tileId === undefined) return;

    const lcdc = e.lcdc || 0;
    const tallSprites = (lcdc & 0x04) !== 0;
    const tileH = tallSprites ? 16 : 8;

    canvas.width = 8;
    canvas.height = tileH;
    canvas.style.width = `${8 * 3}px`;
    canvas.style.height = `${tileH * 3}px`;
    canvas.style.display = 'block';

    try {
      const vram = this.store.getVramAt(entry);
      if (!vram) return;
      const vramArr = new Uint8Array(vram.buffer || vram);

      const flipX = (attr & 0x20) !== 0;
      const flipY = (attr & 0x40) !== 0;

      const ctx = canvas.getContext('2d');
      const imgData = ctx.createImageData(8, tileH);
      const palette = [[0xe0,0xf8,0xd0], [0x88,0xc0,0x70], [0x34,0x68,0x56], [0x08,0x18,0x20]];

      const tilesNeeded = tallSprites ? 2 : 1;
      const baseTile = tallSprites ? (tileId & 0xFE) : tileId;

      for (let t = 0; t < tilesNeeded; t++) {
        const ti = baseTile + (flipY && tallSprites ? (1 - t) : t);
        const tileBase = ti * 16;
        for (let row = 0; row < 8; row++) {
          const srcRow = flipY ? (7 - row) : row;
          const lo = vramArr[tileBase + srcRow * 2] || 0;
          const hi = vramArr[tileBase + srcRow * 2 + 1] || 0;
          for (let col = 0; col < 8; col++) {
            const srcCol = flipX ? col : (7 - col);
            const color = ((hi >> srcCol) & 1) << 1 | ((lo >> srcCol) & 1);
            const px = flipX ? (7 - col) : col;
            const py = t * 8 + row;
            const off = (py * 8 + px) * 4;
            if (color === 0) {
              // Transparent for sprites
              imgData.data[off + 3] = 0;
            } else {
              const [r, g, b] = palette[color];
              imgData.data[off] = r;
              imgData.data[off + 1] = g;
              imgData.data[off + 2] = b;
              imgData.data[off + 3] = 255;
            }
          }
        }
      }

      ctx.putImageData(imgData, 0, 0);
    } catch (err) {
      console.error('Failed to render sprite preview:', err);
    }
  }

  render() {
    if (!this.store?.hasVramData?.()) return html``;

    const spriteInfo = this._getSpriteInfo();

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
          <canvas id="vram-canvas"></canvas>
          <canvas class="overlay"></canvas>
        </div>
        ${spriteInfo ? html`
          <div class="sprite-preview">
            <canvas id="sprite-preview-canvas"></canvas>
            <span>Sprite ${spriteInfo.idx}: tile 0x${spriteInfo.tileId.toString(16).padStart(2,'0')}
              x:${spriteInfo.x} attr:0x${spriteInfo.attr.toString(16).padStart(2,'0')}</span>
          </div>
        ` : ''}
        <div class="label">
          ${this._tab === 'tiles' ? '384 tiles (16×24 grid)' :
            this._tab === 'bg' ? 'Background tilemap (32×32)' :
            'Window tilemap (32×32)'}
        </div>
      </div>
    `;
  }

  _getSpriteInfo() {
    if (this.hoveredSprite == null || this.hoveredSprite < 0 || this.currentIndex == null) return null;
    const e = this.store?.entry(this.currentIndex);
    if (!e) return null;
    const idx = this.hoveredSprite;
    const tileId = e[`oam${idx}_id`];
    const x = e[`oam${idx}_x`];
    const attr = e[`oam${idx}_attr`];
    if (tileId === undefined) return null;
    return { idx, tileId, x, attr };
  }
}

customElements.define('vram-viewer', VramViewer);
