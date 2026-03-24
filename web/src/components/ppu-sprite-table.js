import { LitElement, html, css } from 'lit';

/**
 * Displays the 10 OAM sprite slots at the current trace cursor position.
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
      font-size: 0.7rem;
      font-family: var(--mono);
    }
    .title {
      font-weight: 600;
      color: var(--accent);
      margin-bottom: 4px;
    }
    table {
      border-collapse: collapse;
      width: 100%;
    }
    th {
      text-align: left;
      color: var(--text-muted);
      font-weight: 500;
      padding: 1px 4px;
      border-bottom: 1px solid var(--border);
    }
    td {
      padding: 1px 4px;
      white-space: nowrap;
    }
    tr.inactive td { color: var(--text-muted); opacity: 0.4; }
    .flag { font-size: 0.6rem; }
    .flag-on { color: var(--accent); }
    .flag-off { color: var(--text-muted); opacity: 0.3; }
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
      if (x === undefined) return html``; // no PPU fields in trace
      sprites.push({ i, x, id, attr });
    }

    return html`
      <div class="wrap">
        <div class="title">OAM Sprites</div>
        <table>
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
      </div>
    `;
  }

  _hex(v) {
    return '0x' + (v || 0).toString(16).padStart(2, '0');
  }
}

customElements.define('ppu-sprite-table', PpuSpriteTable);
