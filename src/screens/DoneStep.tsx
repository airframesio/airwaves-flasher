import { CheckCircle2, XCircle, RotateCcw, ExternalLink, AlertTriangle } from 'lucide-react';
import { useFlasher } from '../store';
import { openExternal } from '../lib/api';

export function DoneStep() {
  const { flashError, flashWarning, drive, customization, source, logs, reset } = useFlasher();
  const ok = !flashError;

  const hostname =
    customization.enabled && customization.hostname ? customization.hostname : 'airwaves';
  const dashboardUrl = `http://${hostname}.local`;
  const deviceName =
    source?.kind === 'catalog' ? source.image.name : 'Airwaves OS';

  return (
    <div className="stage__inner">
      <div className="result">
        <div className={`result__badge ${ok ? 'result__badge--ok' : 'result__badge--err'}`}>
          {ok ? <CheckCircle2 size={48} strokeWidth={1.4} /> : <XCircle size={48} strokeWidth={1.4} />}
        </div>

        <h1 className="result__title">{ok ? 'Ready to fly' : 'Flash failed'}</h1>
        <p className="result__msg">
          {ok ? (
            <>
              {deviceName} was written and verified on{' '}
              <span className="mono">{drive?.device}</span>. Your card is ready.
            </>
          ) : (
            flashError
          )}
        </p>

        {ok && flashWarning && (
          <div
            className="banner banner--warn"
            style={{ maxWidth: 520, margin: '1.4rem auto 0', textAlign: 'left' }}
          >
            <AlertTriangle size={18} />
            <div>{flashWarning}</div>
          </div>
        )}

        {ok ? (
          <div className="nextsteps">
            <div className="nextsteps__head">Next steps</div>
            <div className="nextstep">
              <span className="nextstep__n">1</span>
              <p>Remove the card or drive — it’s already been ejected safely.</p>
            </div>
            <div className="nextstep">
              <span className="nextstep__n">2</span>
              <p>
                Insert it into your device and power on. First boot takes a minute or two while it
                expands the filesystem and starts the feeder stack.
              </p>
            </div>
            <div className="nextstep">
              <span className="nextstep__n">3</span>
              <p>
                Open <code>{dashboardUrl}</code> on the same network to finish setup — name your
                station, set its location and choose what to feed.
              </p>
            </div>
          </div>
        ) : (
          logs.length > 0 && (
            <details className="logdrawer" open>
              <summary>Activity log</summary>
              <pre>{logs.join('\n')}</pre>
            </details>
          )
        )}

        <div style={{ display: 'flex', gap: '0.6rem', marginTop: '1.8rem' }}>
          <button className="btn btn--primary" onClick={reset}>
            <RotateCcw size={15} /> Flash another
          </button>
          {ok && (
            <button className="btn btn--ghost" onClick={() => openExternal(dashboardUrl)}>
              <ExternalLink size={15} /> Open dashboard
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
