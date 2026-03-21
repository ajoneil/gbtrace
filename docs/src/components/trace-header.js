import { LitElement, html, css } from 'lit';

export class TraceHeader extends LitElement {
  static styles = css`
    :host { display: block; }
    .header-grid {
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 4px 16px;
      font-size: 0.85rem;
      padding: 16px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
    }
    .label {
      color: var(--text-muted);
      text-align: right;
    }
    .value {
      font-family: var(--mono);
      word-break: break-all;
    }
    .fields {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
    }
    .field-tag {
      background: var(--accent-subtle);
      color: var(--accent);
      padding: 1px 6px;
      border-radius: 4px;
      font-size: 0.8rem;
      font-family: var(--mono);
    }
  `;

  static properties = {
    header: { type: Object },
    entryCount: { type: Number },
    filename: { type: String },
  };

  render() {
    if (!this.header) return '';
    const h = this.header;
    return html`
      <div class="header-grid">
        <span class="label">File</span>
        <span class="value">${this.filename || '?'}</span>
        <span class="label">Emulator</span>
        <span class="value">${h.emulator} ${h.emulator_version}</span>
        <span class="label">Model</span>
        <span class="value">${h.model}</span>
        <span class="label">Profile</span>
        <span class="value">${h.profile}</span>
        <span class="label">Trigger</span>
        <span class="value">${h.trigger}</span>
        <span class="label">Cy unit</span>
        <span class="value">${h.cy_unit || 'tcycle'}</span>
        <span class="label">Boot ROM</span>
        <span class="value">${h.boot_rom}</span>
        <span class="label">ROM hash</span>
        <span class="value">${h.rom_sha256}</span>
        <span class="label">Entries</span>
        <span class="value">${this.entryCount?.toLocaleString() || '?'}</span>
        <span class="label">Fields</span>
        <span class="value">
          <span class="fields">
            ${(h.fields || []).map(f => html`<span class="field-tag">${f}</span>`)}
          </span>
        </span>
      </div>
    `;
  }
}

customElements.define('trace-header', TraceHeader);
