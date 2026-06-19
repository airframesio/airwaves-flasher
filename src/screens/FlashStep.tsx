import { useMemo } from 'react';
import { XCircle } from 'lucide-react';
import { useFlasher } from '../store';
import type { FlashPhase } from '../types';
import { formatBytes, formatDuration, formatSpeed } from '../lib/format';

const ALL_PHASES: { key: FlashPhase; label: string }[] = [
  { key: 'preparing', label: 'Prepare' },
  { key: 'downloading', label: 'Download' },
  { key: 'writing', label: 'Write' },
  { key: 'verifying', label: 'Verify' },
  { key: 'configuring', label: 'Configure' },
  { key: 'finalizing', label: 'Finalize' },
];

const R = 100;
const C = 2 * Math.PI * R;

export function FlashStep() {
  const { progress, logs, source, settings, customization, cancelFlash } = useFlasher();
  const p = progress;

  // which phases are relevant to this run
  const phases = useMemo(
    () =>
      ALL_PHASES.filter((ph) => {
        if (ph.key === 'downloading') return source?.kind === 'catalog';
        if (ph.key === 'verifying') return settings.verifyAfterWrite;
        if (ph.key === 'configuring') return customization.enabled;
        return true;
      }),
    [source, settings.verifyAfterWrite, customization.enabled],
  );

  const phaseIdx = phases.findIndex((x) => x.key === p?.phase);
  const frac = p?.fraction ?? 0;
  const pct = Math.round(frac * 100);
  const dash = C * (1 - frac);
  const canCancel = p && !['finalizing', 'done', 'error', 'cancelled'].includes(p.phase);

  return (
    <div className="stage__inner">
      <div className="flash">
        <div className="gauge">
          <svg viewBox="0 0 220 220">
            <defs>
              <linearGradient id="aw-grad" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0%" stopColor="#ffb347" />
                <stop offset="52%" stopColor="#fc4a67" />
                <stop offset="100%" stopColor="#8e54e9" />
              </linearGradient>
            </defs>
            <circle className="gauge__track" cx="110" cy="110" r={R} />
            <circle
              className="gauge__arc"
              cx="110"
              cy="110"
              r={R}
              strokeDasharray={C}
              strokeDashoffset={dash}
            />
          </svg>
          <div className="gauge__center">
            <div>
              <div className="gauge__pct">{pct}%</div>
              <div className="gauge__phase">{p?.phase ?? 'preparing'}</div>
            </div>
          </div>
        </div>

        <div className="flash__msg">{p?.message ?? 'Preparing…'}</div>

        <div className="flash__stats">
          <div className="flash__stat mono">
            <span>Written</span>
            {formatBytes(p?.bytesProcessed ?? 0)} / {formatBytes(p?.bytesTotal ?? 0)}
          </div>
          <div className="flash__stat mono">
            <span>Speed</span>
            {formatSpeed(p?.speedBps ?? 0)}
          </div>
          <div className="flash__stat mono">
            <span>ETA</span>
            {p?.etaPending ? 'Pending' : formatDuration(p?.etaSecs ?? 0)}
          </div>
        </div>

        <div className="phaserow">
          {phases.map((ph, i) => (
            <span
              key={ph.key}
              className={`phasechip ${i === phaseIdx ? 'is-active' : ''} ${
                phaseIdx >= 0 && i < phaseIdx ? 'is-done' : ''
              }`}
            >
              {ph.label}
            </span>
          ))}
        </div>

        {logs.length > 0 && (
          <details className="logdrawer">
            <summary>Activity log</summary>
            <pre>{logs.join('\n')}</pre>
          </details>
        )}

        {canCancel && (
          <button className="btn btn--danger" style={{ marginTop: '1.6rem' }} onClick={() => cancelFlash()}>
            <XCircle size={15} /> Cancel
          </button>
        )}
      </div>
    </div>
  );
}
