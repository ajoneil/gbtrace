import { LitElement, html, css } from 'lit';

/**
 * Visualizes the APU audio pipeline for all 4 channels.
 *
 * Shows per-channel: active state, frequency timer progress,
 * waveform with current phase, envelope volume, and the mixer
 * output routing.
 *
 * Reads ch{1-4}_active, ch{1-4}_freq_cnt, ch{1-2}_env_vol,
 * ch{1-2}_phase, ch3_wave_idx, ch3_sample, ch4_lfsr, ch4_env_vol,
 * nr{10-52} from the trace entry.
 */
export class ApuVisualizer extends LitElement {
  static properties = {
    store: { type: Object },
    cursorIndex: { type: Number },
    _entry: { state: true },
  };

  static styles = css`
    :host { display: block; }
    .apu {
      border: 1px solid var(--border);
      border-radius: 8px;
      background: var(--bg-surface);
      font-size: 0.7rem;
      font-family: var(--mono);
      overflow: hidden;
    }
    .apu-header {
      padding: 6px 8px;
      border-bottom: 1px solid var(--border);
      font-weight: 600;
      color: var(--accent);
      font-size: 0.65rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .master-vol {
      color: var(--text-muted);
      font-weight: normal;
      text-transform: none;
    }
    .channel {
      display: flex;
      align-items: center;
      gap: 0;
      padding: 6px 8px;
    }
    .channel + .channel {
      border-top: 1px solid var(--border);
    }
    .channel.inactive {
      opacity: 0.3;
    }
    .ch-label {
      width: 32px;
      font-weight: 600;
      font-size: 0.65rem;
      flex-shrink: 0;
    }
    .ch-label.on { color: var(--accent); }
    .ch-label.off { color: var(--text-muted); }
    .stage {
      display: flex;
      flex-direction: column;
      gap: 2px;
      padding: 0 6px;
    }
    .stage + .stage {
      border-left: 1px solid var(--border);
    }
    .stage-label {
      font-size: 0.55rem;
      color: var(--text-muted);
      text-transform: uppercase;
    }
    .arrow {
      color: var(--text-muted);
      font-size: 0.8rem;
      padding: 0 2px;
    }
    /* Frequency timer progress bar */
    .freq-bar {
      width: 60px;
      height: 10px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 2px;
      overflow: hidden;
      position: relative;
    }
    .freq-fill {
      height: 100%;
      background: var(--accent);
      opacity: 0.6;
    }
    .freq-val {
      font-size: 0.55rem;
      color: var(--text-muted);
    }
    /* Duty cycle display (ch1/ch2) */
    .duty {
      display: flex;
      gap: 0;
    }
    .duty-step {
      width: 7px;
      height: 12px;
      border: 0.5px solid var(--border);
    }
    .duty-high { background: var(--accent); opacity: 0.7; }
    .duty-low { background: var(--bg); }
    .duty-current { outline: 2px solid var(--text); outline-offset: -1px; }
    /* Wave table display (ch3) */
    .wave {
      display: flex;
      gap: 0;
      align-items: flex-end;
      height: 16px;
    }
    .wave-sample {
      width: 3px;
      background: var(--accent);
      opacity: 0.5;
      min-height: 1px;
    }
    .wave-sample.current {
      opacity: 1;
      background: var(--text);
    }
    /* LFSR display (ch4) */
    .lfsr {
      display: flex;
      gap: 0;
    }
    .lfsr-bit {
      width: 4px;
      height: 10px;
    }
    .lfsr-1 { background: var(--accent); opacity: 0.6; }
    .lfsr-0 { background: var(--bg); }
    /* Volume bar */
    .vol-bar {
      width: 8px;
      height: 32px;
      background: var(--bg);
      border: 1px solid var(--border);
      border-radius: 2px;
      display: flex;
      flex-direction: column-reverse;
      overflow: hidden;
    }
    .vol-fill {
      width: 100%;
      background: var(--accent);
      transition: height 0.05s;
    }
    .vol-val {
      font-size: 0.6rem;
      color: var(--text);
      font-weight: 600;
      text-align: center;
      min-width: 12px;
    }
    .vol-section {
      display: flex;
      align-items: center;
      gap: 3px;
    }
    /* Mixer routing */
    .routing {
      display: flex;
      gap: 2px;
      font-size: 0.55rem;
    }
    .route-on { color: var(--accent); font-weight: 600; }
    .route-off { color: var(--text-muted); opacity: 0.3; }
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
    if (!this._entry || this._entry.ch1_active === undefined) return html``;
    const e = this._entry;

    const masterVol = e.master_vol ?? 0;
    const soundPan = e.sound_pan ?? 0;
    const volL = (masterVol >> 4) & 7;
    const volR = masterVol & 7;

    return html`
      <div class="apu">
        <div class="apu-header">
          <span>APU Channels</span>
          <span class="master-vol">vol L:${volL} R:${volR}</span>
        </div>
        ${this._renderPulseChannel('CH1', e.ch1_active, e.ch1_freq_cnt,
          e.ch1_freq_lo, e.ch1_freq_hi, e.ch1_phase, e.ch1_env_vol,
          (e.ch1_duty_len >> 6) & 3, soundPan, 0)}
        ${this._renderPulseChannel('CH2', e.ch2_active, e.ch2_freq_cnt,
          e.ch2_freq_lo, e.ch2_freq_hi, e.ch2_phase, e.ch2_env_vol,
          (e.ch2_duty_len >> 6) & 3, soundPan, 1)}
        ${this._renderWaveChannel(e, soundPan)}
        ${this._renderNoiseChannel(e, soundPan)}
      </div>
    `;
  }

  _renderPulseChannel(label, active, freqCnt, freqLo, freqHi, phase, envVol, duty, soundPan, chIdx) {
    const period = ((freqHi & 7) << 8) | (freqLo ?? 0);
    const dutyPattern = [
      [0,0,0,0,0,0,0,1], // 12.5%
      [1,0,0,0,0,0,0,1], // 25%
      [1,0,0,0,0,1,1,1], // 50%
      [0,1,1,1,1,1,1,0], // 75%
    ][duty & 3];

    const freqPct = period > 0 ? Math.min(100, ((freqCnt ?? 0) / period) * 100) : 0;
    const routing = this._channelRouting(soundPan, chIdx);

    return html`
      <div class="channel ${active ? '' : 'inactive'}">
        <span class="ch-label ${active ? 'on' : 'off'}">${label}</span>

        <div class="stage">
          <div class="stage-label">freq</div>
          <div class="freq-bar">
            <div class="freq-fill" style="width:${freqPct}%"></div>
          </div>
          <div class="freq-val">${period}</div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">duty</div>
          <div class="duty">
            ${dutyPattern.map((v, i) => html`
              <div class="duty-step ${v ? 'duty-high' : 'duty-low'} ${i === (phase ?? 0) ? 'duty-current' : ''}"></div>
            `)}
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">vol</div>
          <div class="vol-section">
            <div class="vol-bar">
              <div class="vol-fill" style="height:${((envVol ?? 0) / 15) * 100}%"></div>
            </div>
            <span class="vol-val">${envVol ?? 0}</span>
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">mix</div>
          <div class="routing">${routing}</div>
        </div>
      </div>
    `;
  }

  _renderWaveChannel(e, soundPan) {
    const active = e.ch3_active;
    const freqCnt = e.ch3_freq_cnt ?? 0;
    const period = ((e.ch3_freq_hi & 7) << 8) | (e.ch3_freq_lo ?? 0);
    const waveIdx = e.ch3_wave_idx ?? 0;
    const sample = e.ch3_sample ?? 0;
    const volShift = (e.ch3_vol >> 5) & 3; // 0=mute, 1=100%, 2=50%, 3=25%
    const volLabels = ['0%', '100%', '50%', '25%'];
    const freqPct = period > 0 ? Math.min(100, (freqCnt / period) * 100) : 0;
    const routing = this._channelRouting(nr51, 2);

    // Build wave table from NR30-region — we don't have wave RAM in the trace,
    // but we have the current sample value and position
    const waveSamples = [];
    for (let i = 0; i < 32; i++) {
      // We only know the current sample, show placeholder for others
      waveSamples.push(i === waveIdx ? sample : -1);
    }

    return html`
      <div class="channel ${active ? '' : 'inactive'}">
        <span class="ch-label ${active ? 'on' : 'off'}">CH3</span>

        <div class="stage">
          <div class="stage-label">freq</div>
          <div class="freq-bar">
            <div class="freq-fill" style="width:${freqPct}%"></div>
          </div>
          <div class="freq-val">${period}</div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">wave [${waveIdx}]</div>
          <div class="wave">
            ${waveSamples.map((s, i) => html`
              <div class="wave-sample ${i === waveIdx ? 'current' : ''}"
                   style="height:${s >= 0 ? (s / 15) * 16 : 2}px;
                          ${s < 0 ? 'opacity:0.15' : ''}"></div>
            `)}
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">vol</div>
          <div class="vol-section">
            <span class="vol-val">${volLabels[volShift]}</span>
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">mix</div>
          <div class="routing">${routing}</div>
        </div>
      </div>
    `;
  }

  _renderNoiseChannel(e, soundPan) {
    const active = e.ch4_active;
    const envVol = e.ch4_env_vol ?? 0;
    const lfsr = e.ch4_lfsr ?? 0;
    const routing = this._channelRouting(soundPan, 3);

    // Show 16 bits of LFSR
    const lfsrBits = [];
    for (let i = 15; i >= 0; i--) {
      lfsrBits.push((lfsr >> i) & 1);
    }

    const ch4Freq = e.ch4_freq ?? 0;
    const divider = ch4Freq & 7;
    const width = (ch4Freq >> 3) & 1;
    const shift = (ch4Freq >> 4) & 0xF;

    return html`
      <div class="channel ${active ? '' : 'inactive'}">
        <span class="ch-label ${active ? 'on' : 'off'}">CH4</span>

        <div class="stage">
          <div class="stage-label">noise</div>
          <div class="freq-val">div:${divider} sh:${shift} ${width ? '7bit' : '15bit'}</div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">lfsr</div>
          <div class="lfsr">
            ${lfsrBits.map(b => html`
              <div class="lfsr-bit ${b ? 'lfsr-1' : 'lfsr-0'}"></div>
            `)}
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">vol</div>
          <div class="vol-section">
            <div class="vol-bar">
              <div class="vol-fill" style="height:${(envVol / 15) * 100}%"></div>
            </div>
            <span class="vol-val">${envVol}</span>
          </div>
        </div>

        <span class="arrow">\u2192</span>

        <div class="stage">
          <div class="stage-label">mix</div>
          <div class="routing">${routing}</div>
        </div>
      </div>
    `;
  }

  _channelRouting(nr51, chIdx) {
    const right = (nr51 >> chIdx) & 1;
    const left = (nr51 >> (chIdx + 4)) & 1;
    return html`
      <span class="${left ? 'route-on' : 'route-off'}">L</span>
      <span class="${right ? 'route-on' : 'route-off'}">R</span>
    `;
  }
}

customElements.define('apu-visualizer', ApuVisualizer);
