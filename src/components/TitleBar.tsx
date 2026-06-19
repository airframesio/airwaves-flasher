import { Minus, Square, X, Sun, Moon, Settings, RefreshCw } from 'lucide-react';
import { Brand } from './Brand';
import { platform, windowControls } from '../lib/api';
import { useFlasher } from '../store';

export function TitleBar({ onSettings }: { onSettings: () => void }) {
  const theme = useFlasher((s) => s.settings.theme);
  const updateSettings = useFlasher((s) => s.updateSettings);
  const loadCatalog = useFlasher((s) => s.loadCatalog);
  const isMac = platform === 'macos';

  return (
    <div className="titlebar" data-platform={platform}>
      <div className="titlebar__drag" />

      {!isMac && (
        <div className="titlebar__brand">
          <Brand sub={false} />
        </div>
      )}
      {isMac && <span className="titlebar__title titlebar__title--center">Airwaves OS Flasher</span>}

      <div className="titlebar__tools">
        <button
          className="iconbtn"
          title="Refresh catalog"
          onClick={() => loadCatalog(true)}
        >
          <RefreshCw size={15} />
        </button>
        <button
          className="iconbtn"
          title={theme === 'dark' ? 'Light mode' : 'Dark mode'}
          onClick={() => updateSettings({ theme: theme === 'dark' ? 'light' : 'dark' })}
        >
          {theme === 'dark' ? <Sun size={15} /> : <Moon size={15} />}
        </button>
        <button className="iconbtn" title="Settings" onClick={onSettings}>
          <Settings size={15} />
        </button>

        {!isMac && (
          <>
            <span style={{ width: 6 }} />
            <button className="iconbtn winctl" title="Minimize" onClick={() => windowControls.minimize()}>
              <Minus size={14} />
            </button>
            <button className="iconbtn winctl" title="Maximize" onClick={() => windowControls.toggleMaximize()}>
              <Square size={11} />
            </button>
            <button className="iconbtn winctl winctl--close" title="Close" onClick={() => windowControls.close()}>
              <X size={14} />
            </button>
          </>
        )}
      </div>
    </div>
  );
}
