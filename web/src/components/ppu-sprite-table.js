import { LitElement, html, css } from 'lit';

/**
 * Displays the 10 OAM sprite slots in a compact two-column layout.
 * Reads oam{0-9}_{x,id,attr} fields from the trace entry.
 */
export class PpuSpriteTable extends LitElement {
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
      font-size: 0.65rem;
      font-family: var(--mono);
    }
    .title {
      font-weight: 600;
      color: var(--accent);
      margin-bottom: 4px;
      font-size: 0.6rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }
    .columns {
      display: flex;
      gap: 8px;
    }
    table {
      border-collapse: collapse;
      table-layout: fixed;
    }
    col.idx { width: 16px; }
    col.xpos { width: 28px; }
    col.tile { width: 32px; }
    col.attr { width: 40px; }
    th {
      text-align: right;
      color: var(--text-muted);
      font-weight: 500;
      padding: 1px 3px;
      border-bottom: 1px solid var(--border);
    }
    td {
      padding: 1px 3px;
      white-space: nowrap;
      text-align: right;
    }
    tr.inactive td { color: var(--text-muted); opacity: 0.3; }
    .flag { font-size: 0.55rem; }
    .flag-on { color: var(--accent); }
    .flag-off { color: var(--text-muted); opacity: 0.3; }

    .stat-bar {
      margin-top: 6px;
      padding-top: 6px;
      border-top: 1px solid var(--border);
      display: flex;
      flex-direction: column;
      gap: 3px;
    }
    .stat-row {
      display: flex;
      gap: 6px;
      flex-wrap: wrap;
      align-items: baseline;
    }
    .stat-label { color: var(--text-muted); }
    .stat-val { color: var(--text); }
    .stat-flag-on { color: var(--accent); font-weight: 600; }
    .stat-flag-off { color: var(--text-muted); opacity: 0.3; }
    .mode-badge {
      padding: 0 4px;
      border-radius: 3px;
      font-weight: 600;
      font-size: 0.6rem;
    }
    .mode-0 { background: #1a3a1a; color: #4caf50; }
    .mode-1 { background: #3a1a1a; color: #f44336; }
    .mode-2 { background: #1a1a3a; color: #42a5f5; }
    .mode-3 { background: #3a3a1a; color: #ffb74d; }
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
        });
      }
    }
  }

  render() {
    if (!this._entry) return html``;
    const e = this._entry;

    const sprites = [];
    for (let i = 0; i < 10; i++) {
      const x = e[`oam${i}_x`];
      const id = e[`oam${i}_id`];
      const attr = e[`oam${i}_attr`];
      if (x === undefined) return html``;
      sprites.push({ i, x, id, attr });
    }

    const left = sprites.slice(0, 5);
    const right = sprites.slice(5);

    return html`
      <div class="wrap">
        <div class="title">OAM Sprites</div>
        <div class="columns">
          ${this._renderColumn(left)}
          ${this._renderColumn(right)}
        </div>
        ${this._renderStatInfo(e)}
      </div>
    `;
  }

  _renderColumn(sprites) {
    return html`
      <table>
        <colgroup>
          <col class="idx"><col class="xpos"><col class="tile"><col class="attr">
        </colgroup>
        <tr><th>#</th><th>X</th><th>Tile</th><th>Attr</th></tr>
        ${sprites.map(s => {
          const active = s.x !== 0;
          const pri = (s.attr >> 3) & 1;
          const yf = (s.attr >> 2) & 1;
          const xf = (s.attr >> 1) & 1;
          const pal = s.attr & 1;
          return html`
            <tr class="${active ? '' : 'inactive'}">
              <td>${s.i}</td>
              <td>${s.x}</td>
              <td>${this._hex(s.id)}</td>
              <td>
                <span class="flag ${pri ? 'flag-on' : 'flag-off'}">P</span>
                <span class="flag ${yf ? 'flag-on' : 'flag-off'}">Y</span>
                <span class="flag ${xf ? 'flag-on' : 'flag-off'}">X</span>
                <span class="flag">${pal ? '1' : '0'}</span>
              </td>
            </tr>`;
        })}
      </table>
    `;
  }

  _renderStatInfo(e) {
    if (e.lcdc === undefined && e.stat === undefined) return html``;

    const mode = e.stat !== undefined ? (e.stat & 3) : null;
    const modeLabels = ['HBlank', 'VBlank', 'OAM', 'Draw'];

    // LCDC bit decode
    const lcdc = e.lcdc ?? 0;
    const lcdOn = (lcdc >> 7) & 1;
    const winMap = (lcdc >> 6) & 1;
    const winEn = (lcdc >> 5) & 1;
    const tileData = (lcdc >> 4) & 1;
    const bgMap = (lcdc >> 3) & 1;
    const objSize = (lcdc >> 2) & 1;
    const objEn = (lcdc >> 1) & 1;
    const bgEn = lcdc & 1;

    return html`
      <div class="stat-bar">
        <div class="stat-row">
          ${mode !== null ? html`
            <span class="mode-badge mode-${mode}">${modeLabels[mode]}</span>
          ` : ''}
          <span class="${lcdOn ? 'stat-flag-on' : 'stat-flag-off'}">LCD</span>
          <span class="${bgEn ? 'stat-flag-on' : 'stat-flag-off'}">BG</span>
          <span class="${objEn ? 'stat-flag-on' : 'stat-flag-off'}">OBJ</span>
          <span class="${winEn ? 'stat-flag-on' : 'stat-flag-off'}">WIN</span>
          <span class="${objSize ? 'stat-flag-on' : 'stat-flag-off'}">8x16</span>
        </div>
      </div>
    `;
  }

  _hex(v) {
    return '0x' + (v || 0).toString(16).padStart(2, '0');
  }
}

customElements.define('ppu-sprite-table', PpuSpriteTable);
