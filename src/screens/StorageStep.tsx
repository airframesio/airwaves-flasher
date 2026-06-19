import { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  HardDrive,
  Lock,
  MemoryStick,
  RotateCw,
  Usb,
  Search,
} from 'lucide-react';
import { useFlasher } from '../store';
import type { DriveInfo } from '../types';
import { listDrives } from '../lib/api';
import { formatBytes } from '../lib/format';

function driveIcon(bus?: string) {
  const b = (bus ?? '').toUpperCase();
  if (b.includes('USB')) return Usb;
  if (b.includes('SD') || b.includes('MMC')) return MemoryStick;
  return HardDrive;
}

export function StorageStep() {
  const { drive, source, chooseDrive } = useFlasher();
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const minBytes = useMemo(() => {
    if (source?.kind === 'catalog')
      return source.image.minStorageBytes ?? source.image.extractSize;
    if (source?.kind === 'local') return source.file.extractSize ?? source.file.size;
    return 0;
  }, [source]);

  async function refresh() {
    try {
      const list = await listDrives();
      setDrives(list);
      // drop selection if the device went away
      const sel = useFlasher.getState().drive;
      if (sel && !list.some((d) => d.id === sel.id)) chooseDrive(null);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 2500);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="stage__inner">
      <div className="stage__head">
        <span className="kicker">Step 2 — Storage device</span>
        <h1 className="stage__title">Choose where to write</h1>
        <p className="stage__lede">
          Select the SD card or USB drive to flash. Internal and system disks are hidden for
          safety. The card is detected automatically — insert it now if you haven’t.
        </p>
      </div>

      <div className="banner banner--warn">
        <AlertTriangle size={18} />
        <div>
          <strong>Everything on the selected device will be erased.</strong> Double-check the
          name and capacity before you continue.
        </div>
      </div>

      <div
        style={{
          display: 'flex',
          justifyContent: 'flex-end',
          marginBottom: '0.9rem',
        }}
      >
        <button className="btn btn--ghost btn--small" onClick={refresh}>
          <RotateCw size={13} /> Refresh
        </button>
      </div>

      {loading && drives.length === 0 && (
        <div className="center-load">
          <span className="spinner" /> Scanning for drives…
        </div>
      )}

      {!loading && drives.length === 0 && (
        <div className="empty">
          <Search size={42} strokeWidth={1.2} />
          <div className="empty__title">No removable drives found</div>
          <p>Insert an SD card or USB drive, then press Refresh.</p>
        </div>
      )}

      {drives.length > 0 && (
        <div className="cardgrid">
          {drives.map((d) => {
            const Ico = driveIcon(d.busType);
            const tooSmall = minBytes > 0 && d.sizeBytes > 0 && d.sizeBytes < minBytes;
            const blocked = d.system || d.readOnly;
            const disabled = blocked || tooSmall;
            const selected = drive?.id === d.id;
            let note: string | null = null;
            if (d.system) note = 'System disk — protected';
            else if (d.readOnly) note = 'Read-only';
            else if (tooSmall) note = `Too small — needs ≥ ${formatBytes(minBytes)}`;

            return (
              <button
                key={d.id}
                className={`selectcard ${selected ? 'is-selected' : ''} ${
                  disabled ? 'is-disabled' : ''
                }`}
                disabled={disabled}
                onClick={() => chooseDrive(d)}
              >
                <span className="selectcard__check">
                  <Check size={20} />
                </span>
                <span className="selectcard__icon">
                  {blocked ? <Lock size={20} /> : <Ico size={22} strokeWidth={1.6} />}
                </span>
                <span className="selectcard__body">
                  <span className="selectcard__title">{d.description || 'Storage device'}</span>
                  <span className="selectcard__desc mono">{d.device}</span>
                  <span className="selectcard__meta">
                    <span style={{ color: 'var(--cream)' }}>{formatBytes(d.sizeBytes)}</span>
                    {d.busType && (
                      <>
                        <span>·</span>
                        <span>{d.busType}</span>
                      </>
                    )}
                    {d.mountpoints.length > 0 && (
                      <>
                        <span>·</span>
                        <span>{d.mountpoints[0]}</span>
                      </>
                    )}
                    {note && (
                      <>
                        <span>·</span>
                        <span style={{ color: 'var(--ember)' }}>{note}</span>
                      </>
                    )}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
