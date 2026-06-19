import { useEffect, useState } from 'react';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { AlertTriangle, ArrowLeft, ArrowRight, Flame, X } from 'lucide-react';
import { useFlasher } from './store';
import { TitleBar } from './components/TitleBar';
import { StepRail } from './components/StepRail';
import { ImageStep } from './screens/ImageStep';
import { StorageStep } from './screens/StorageStep';
import { CustomizeStep } from './screens/CustomizeStep';
import { FlashStep } from './screens/FlashStep';
import { DoneStep } from './screens/DoneStep';
import { formatBytes } from './lib/format';
import { DEFAULT_CATALOG_URL } from './types';
import { Select } from './components/Select';
import './styles/app.css';

export default function App() {
  const store = useFlasher();
  const { step, source, drive } = store;
  const [confirm, setConfirm] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const reduce = useReducedMotion();

  useEffect(() => {
    store.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const sourceLabel =
    source?.kind === 'catalog'
      ? source.image.name
      : source?.kind === 'local'
        ? source.file.name
        : null;

  const screen = {
    image: <ImageStep />,
    storage: <StorageStep />,
    customize: <CustomizeStep />,
    flash: <FlashStep />,
    done: <DoneStep />,
  }[step];

  const showFooter = step === 'image' || step === 'storage' || step === 'customize';

  return (
    <div className="app atmosphere">
      <div className="glow" />
      <TitleBar onSettings={() => setSettingsOpen(true)} />

      <div className="shell shell--rail">
        <StepRail />

        <div className="main">
          <div className="stage">
            <AnimatePresence mode="wait">
              <motion.div
                key={step}
                initial={reduce ? false : { opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                exit={reduce ? undefined : { opacity: 0, y: -8 }}
                transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
              >
                {screen}
              </motion.div>
            </AnimatePresence>
          </div>

          {showFooter && (
            <div className="footer-bar">
              <div className="footer-bar__summary">
                {sourceLabel ? (
                  <>
                    <span className="dot dot--live" />
                    <span className="mono footer-bar__src">{sourceLabel}</span>
                    {drive && (
                      <>
                        <span style={{ color: 'var(--muted)' }}>→</span>
                        <span className="mono">{drive.device}</span>
                      </>
                    )}
                  </>
                ) : (
                  <span style={{ color: 'var(--muted)' }}>Select an image to begin</span>
                )}
              </div>

              <div className="footer-bar__actions">
                {step !== 'image' && (
                  <button className="btn btn--ghost" onClick={store.back}>
                    <ArrowLeft size={15} /> Back
                  </button>
                )}
                {step === 'image' && (
                  <button className="btn btn--primary" disabled={!source} onClick={store.next}>
                    Next <ArrowRight size={15} className="btn__arrow" />
                  </button>
                )}
                {step === 'storage' && (
                  <button className="btn btn--primary" disabled={!drive} onClick={store.next}>
                    Next <ArrowRight size={15} className="btn__arrow" />
                  </button>
                )}
                {step === 'customize' && (
                  <button
                    className="btn btn--primary"
                    disabled={!source || !drive}
                    onClick={() => setConfirm(true)}
                  >
                    <Flame size={15} /> Write Airwaves OS
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      </div>

      <AnimatePresence>
        {confirm && drive && (
          <ConfirmErase
            onCancel={() => setConfirm(false)}
            onConfirm={() => {
              setConfirm(false);
              void store.beginFlash();
            }}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {settingsOpen && <SettingsSheet onClose={() => setSettingsOpen(false)} />}
      </AnimatePresence>
    </div>
  );
}

function ConfirmErase({ onCancel, onConfirm }: { onCancel: () => void; onConfirm: () => void }) {
  const drive = useFlasher((s) => s.drive)!;
  return (
    <motion.div
      className="modal-scrim"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onCancel}
    >
      <motion.div
        className="modal"
        initial={{ scale: 0.94, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.96, opacity: 0 }}
        transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal__head">
          <AlertTriangle size={22} />
          <span className="modal__title">Erase and write?</span>
        </div>
        <div className="modal__body">
          Everything on this device will be <strong>permanently erased</strong> and replaced with
          Airwaves OS.
          <div className="modal__target">
            {drive.description || 'Storage device'} · {formatBytes(drive.sizeBytes)}
            <br />
            <span style={{ color: 'var(--muted)' }}>{drive.device}</span>
          </div>
        </div>
        <div className="modal__actions">
          <button className="btn btn--ghost" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn btn--danger" onClick={onConfirm}>
            <Flame size={15} /> Erase &amp; write
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

function SettingsSheet({ onClose }: { onClose: () => void }) {
  const { settings, updateSettings } = useFlasher();
  const [url, setUrl] = useState(settings.catalogUrl);

  return (
    <motion.div
      className="modal-scrim"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
    >
      <motion.div
        className="modal"
        initial={{ scale: 0.94, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.96, opacity: 0 }}
        transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal__head" style={{ justifyContent: 'space-between' }}>
          <span className="modal__title">Settings</span>
          <button className="iconbtn" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="modal__body">
          <div className="field">
            <label className="field__label">Catalog URL</label>
            <input
              className="input"
              value={url}
              spellCheck={false}
              onChange={(e) => setUrl(e.target.value)}
              onBlur={() => updateSettings({ catalogUrl: url })}
            />
            <div className="field__hint">
              Where the image catalog is fetched from. Falls back to GitHub Releases if
              unreachable.{' '}
              <button
                className="linkbtn"
                onClick={() => {
                  setUrl(DEFAULT_CATALOG_URL);
                  updateSettings({ catalogUrl: DEFAULT_CATALOG_URL });
                }}
              >
                Reset
              </button>
            </div>
          </div>

          <div className="switchrow">
            <div className="switchrow__text">
              <strong>Verify after writing</strong>
              <p>Read the device back and check it matches. Recommended.</p>
            </div>
            <button
              className={`switch ${settings.verifyAfterWrite ? 'is-on' : ''}`}
              onClick={() => updateSettings({ verifyAfterWrite: !settings.verifyAfterWrite })}
            />
          </div>
          <div className="switchrow">
            <div className="switchrow__text">
              <strong>Eject when done</strong>
              <p>Safely eject the device after a successful write.</p>
            </div>
            <button
              className={`switch ${settings.ejectWhenDone ? 'is-on' : ''}`}
              onClick={() => updateSettings({ ejectWhenDone: !settings.ejectWhenDone })}
            />
          </div>

          <div className="field" style={{ marginTop: '1rem', marginBottom: 0 }}>
            <label className="field__label">Keep downloads cached</label>
            <Select
              ariaLabel="Download cache duration"
              value={String(settings.cacheHours)}
              options={[
                { value: '0', label: 'Off' },
                { value: '1', label: '1 hour' },
                { value: '3', label: '3 hours' },
                { value: '6', label: '6 hours' },
                { value: '12', label: '12 hours' },
                { value: '24', label: '24 hours' },
              ]}
              onChange={(v) => updateSettings({ cacheHours: Number(v) })}
            />
            <div className="field__hint">
              Reuse a downloaded image if you re-flash within this window — no second download
              if a write fails.
            </div>
          </div>
        </div>
        <div className="modal__actions">
          <button className="btn btn--primary" onClick={onClose}>
            Done
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
