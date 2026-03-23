import { LitElement, html, css } from 'lit';

export class TraceHeader extends LitElement {
  static styles = css`
    :host { display: block; }

    .summary {
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 8px 12px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      font-size: 0.8rem;
      font-family: var(--mono);
      cursor: pointer;
      user-select: none;
    }
    .summary:hover { border-color: var(--accent); }
    .summary .toggle {
      color: var(--text-muted);
      font-size: 0.7rem;
      transition: transform 0.15s;
    }
    .summary .toggle.open { transform: rotate(90deg); }
    .summary .filename {
      font-weight: 600;
      color: var(--text);
    }
    .summary .meta {
      color: var(--text-muted);
    }
    .summary .meta strong {
      color: var(--text);
    }
    .summary .entries {
      margin-left: auto;
      color: var(--text-muted);
    }
    .summary .entries strong {
      color: var(--accent);
    }

    .details {
      margin-top: 6px;
      padding: 12px 16px;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 3px 16px;
      font-size: 0.8rem;
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
      font-size: 0.75rem;
      font-family: var(--mono);
    }
  `;

  static properties = {
    header: { type: Object },
    entryCount: { type: Number },
    filename: { type: String },
    _expanded: { state: true },
  };

  constructor() {
    super();
    this._expanded = false;
  }

  render() {
    if (!this.header) return '';
    const h = this.header;
    return html`
      <div class="summary" @click=${() => this._expanded = !this._expanded}>
        <span class="toggle ${this._expanded ? 'open' : ''}">&#9654;</span>
        <span class="filename">${this.filename || '?'}</span>
        <span class="meta">${h.emulator} <strong>${h.model}</strong> ${h.profile}</span>
        <span class="entries"><strong>${this.entryCount?.toLocaleString() || '?'}</strong> entries</span>
      </div>
      ${this._expanded ? html`
        <div class="details">
          <span class="label">Emulator</span>
          <span class="value">${h.emulator} ${h.emulator_version}</span>
          <span class="label">Model</span>
          <span class="value">${h.model}</span>
          <span class="label">Profile</span>
          <span class="value">${h.profile}</span>
          <span class="label">Trigger</span>
          <span class="value">${h.trigger}</span>
          <span class="label">Boot ROM</span>
          <span class="value">${h.boot_rom}</span>
          <span class="label">ROM hash</span>
          <span class="value">${h.rom_sha256}</span>
          <span class="label">Fields</span>
          <span class="value">
            <span class="fields">
              ${(h.fields || []).map(f => html`<span class="field-tag">${f}</span>`)}
            </span>
          </span>
        </div>
      ` : ''}
    `;
  }
}

customElements.define('trace-header', TraceHeader);
