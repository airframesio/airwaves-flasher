import { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  ExternalLink,
  FileUp,
  FolderOpen,
  RotateCw,
  Search,
} from 'lucide-react';
import { useFlasher } from '../store';
import type { Channel, CatalogDevice, CatalogImage } from '../types';
import { Select } from '../components/Select';
import { VendorBadge } from '../components/VendorBadge';
import { BoardThumb } from '../components/BoardThumb';
import { compareVersions, formatBytes, formatDate } from '../lib/format';
import { openExternal, pickLocalImage } from '../lib/api';
import { armbianDevices } from '../data/armbianBoards';

const CHANNELS: Channel[] = ['stable', 'beta', 'dev'];
const MAX_RESULTS = 60;

function mergeDevices(base: CatalogDevice[], over: CatalogDevice[]): CatalogDevice[] {
  const map = new Map(base.map((d) => [d.id, d]));
  for (const d of over) map.set(d.id, { ...(map.get(d.id) ?? d), ...d });
  return [...map.values()];
}

interface VendorInfo {
  name: string;
  total: number;
  available: number;
}

export function ImageStep() {
  const { catalog, catalogLoading, catalogError, source, loadCatalog, chooseImage, chooseLocal } =
    useFlasher();
  const [channel, setChannel] = useState<Channel>('stable');
  const [vendor, setVendor] = useState<string | null>(null);
  const [board, setBoard] = useState<string | null>(
    source?.kind === 'catalog' ? source.image.device : null,
  );
  const [query, setQuery] = useState('');
  const [busyLocal, setBusyLocal] = useState(false);

  const allDevices = useMemo(
    () => mergeDevices(armbianDevices, catalog?.devices ?? []),
    [catalog],
  );

  const imagesFor = useMemo(() => {
    return (deviceId: string, ch: Channel): CatalogImage[] => {
      if (!catalog) return [];
      return catalog.images
        .filter((i) => i.device === deviceId && i.channel === ch)
        .sort((a, b) => compareVersions(b.osVersion, a.osVersion));
    };
  }, [catalog]);

  const hasImage = (id: string) => imagesFor(id, channel).length > 0;

  const availableBoards = useMemo(
    () => allDevices.filter((d) => hasImage(d.id)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [allDevices, channel, catalog],
  );

  const vendors = useMemo(() => {
    const m = new Map<string, VendorInfo>();
    for (const d of allDevices) {
      const v = d.vendor || 'Other';
      const e = m.get(v) ?? { name: v, total: 0, available: 0 };
      e.total += 1;
      if (hasImage(d.id)) e.available += 1;
      m.set(v, e);
    }
    return [...m.values()].sort(
      (a, b) => b.available - a.available || b.total - a.total || a.name.localeCompare(b.name),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allDevices, channel, catalog]);

  // keep the chosen version sensible as channel/board change
  useEffect(() => {
    if (!board || !catalog) return;
    const vs = imagesFor(board, channel);
    if (vs.length === 0) {
      if (source?.kind === 'catalog' && source.image.device === board) useFlasher.getState().clearSource();
      return;
    }
    const valid =
      source?.kind === 'catalog' && source.image.device === board && source.image.channel === channel;
    if (!valid) chooseImage(vs[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel, board, catalog]);

  const localFile = source?.kind === 'local' ? source.file : null;
  const selectedImage = source?.kind === 'catalog' ? source.image : null;
  const versions = board ? imagesFor(board, channel) : [];
  const q = query.trim().toLowerCase();

  const searchResults = useMemo(() => {
    if (!q) return [];
    const matched = allDevices.filter(
      (d) =>
        d.name.toLowerCase().includes(q) ||
        d.id.toLowerCase().includes(q) ||
        (d.vendor ?? '').toLowerCase().includes(q),
    );
    const avail = matched.filter((d) => hasImage(d.id));
    const rest = matched.filter((d) => !hasImage(d.id));
    return [...avail, ...rest].slice(0, MAX_RESULTS);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allDevices, q, channel, catalog]);

  function selectBoard(deviceId: string) {
    const vs = imagesFor(deviceId, channel);
    if (vs.length === 0) return;
    setBoard(deviceId);
    chooseImage(vs[0]);
  }

  async function browse() {
    setBusyLocal(true);
    try {
      const f = await pickLocalImage();
      if (f) {
        setBoard(null);
        chooseLocal(f);
      }
    } finally {
      setBusyLocal(false);
    }
  }

  return (
    <div className="stage__inner">
      <div className="stage__head">
        <span className="kicker">Step 1 — Operating system</span>
        <h1 className="stage__title">Choose your board</h1>
        <p className="stage__lede">
          Boards with a published Airwaves OS image are ready to flash. Browse all{' '}
          {allDevices.length} boards Armbian supports by manufacturer, or search. Already have an{' '}
          <code className="mono">.img.xz</code>? Use a local file.
        </p>
      </div>

      <div className="toolbar">
        <div className="segmented" role="tablist" aria-label="Release channel">
          {CHANNELS.map((c) => (
            <button key={c} className={channel === c ? 'is-active' : ''} onClick={() => setChannel(c)}>
              {c}
            </button>
          ))}
        </div>
        <button className="btn btn--ghost" onClick={browse} disabled={busyLocal}>
          <FolderOpen size={15} /> Use local file
        </button>
      </div>

      <div className="searchbar">
        <Search size={15} />
        <input
          className="searchbar__input"
          placeholder={`Search ${allDevices.length} boards or manufacturers…`}
          value={query}
          spellCheck={false}
          onChange={(e) => setQuery(e.target.value)}
        />
        {query && (
          <button className="iconbtn" onClick={() => setQuery('')} title="Clear">
            ✕
          </button>
        )}
      </div>

      {localFile && <LocalCard name={localFile.name} size={localFile.size} comp={localFile.compression} onClick={browse} />}

      {catalogLoading && (
        <div className="center-load">
          <span className="spinner" /> Fetching catalog…
        </div>
      )}

      {catalogError && !catalogLoading && (
        <div className="banner banner--warn">
          <AlertTriangle size={18} />
          <div>
            <strong>Couldn’t load the online catalog.</strong>
            <div style={{ marginTop: 4, color: 'var(--cream-dim)' }}>{catalogError}</div>
            <div style={{ marginTop: 8, display: 'flex', gap: '0.6rem' }}>
              <button className="btn btn--ghost btn--small" onClick={() => loadCatalog(true)}>
                <RotateCw size={13} /> Retry
              </button>
              <button className="btn btn--ghost btn--small" onClick={browse}>
                <FolderOpen size={13} /> Use local file
              </button>
            </div>
          </div>
        </div>
      )}

      {catalog && !catalogLoading && (
        <>
          {q ? (
            // ---- search results across all boards ----
            <Section title={`${searchResults.length} match${searchResults.length === 1 ? '' : 'es'}`}>
              {searchResults.length === 0 ? (
                <Empty query={query} channel={channel} />
              ) : (
                <div className="cardgrid">
                  {searchResults.map((d) => (
                    <BoardCard
                      key={d.id}
                      dev={d}
                      versions={imagesFor(d.id, channel)}
                      channel={channel}
                      selected={board === d.id && !localFile}
                      onSelect={() => selectBoard(d.id)}
                    />
                  ))}
                </div>
              )}
            </Section>
          ) : vendor ? (
            // ---- one manufacturer's boards ----
            <>
              <button className="backlink" onClick={() => setVendor(null)}>
                <ChevronLeft size={15} /> All manufacturers
              </button>
              <Section title={vendor}>
                <div className="cardgrid">
                  {allDevices
                    .filter((d) => (d.vendor || 'Other') === vendor)
                    .map((d) => (
                      <BoardCard
                        key={d.id}
                        dev={d}
                        versions={imagesFor(d.id, channel)}
                        channel={channel}
                        selected={board === d.id && !localFile}
                        onSelect={() => selectBoard(d.id)}
                      />
                    ))}
                </div>
              </Section>
            </>
          ) : (
            <>
              {availableBoards.length > 0 && (
                <Section title="Ready to flash" hint="Published Airwaves OS images">
                  <div className="cardgrid">
                    {availableBoards.map((d) => (
                      <BoardCard
                        key={d.id}
                        dev={d}
                        versions={imagesFor(d.id, channel)}
                        channel={channel}
                        selected={board === d.id && !localFile}
                        onSelect={() => selectBoard(d.id)}
                      />
                    ))}
                  </div>
                </Section>
              )}

              <Section title="Browse by manufacturer" hint={`${vendors.length} manufacturers`}>
                <div className="vendorgrid">
                  {vendors.map((v) => (
                    <button key={v.name} className="vendorcard" onClick={() => setVendor(v.name)}>
                      <VendorBadge vendor={v.name} />
                      <span className="vendorcard__body">
                        <span className="vendorcard__name">{v.name}</span>
                        <span className="vendorcard__meta">
                          {v.total} board{v.total === 1 ? '' : 's'}
                          {v.available > 0 && <span className="tag tag--ok">{v.available} ready</span>}
                        </span>
                      </span>
                    </button>
                  ))}
                </div>
              </Section>
            </>
          )}

          {board && !localFile && versions.length > 0 && selectedImage && (
            <VersionPanel
              boardId={board}
              boardName={allDevices.find((d) => d.id === board)?.name ?? board}
              boardIcon={allDevices.find((d) => d.id === board)?.icon ?? 'sbc'}
              channel={channel}
              versions={versions}
              selected={selectedImage}
              onSelect={(img) => chooseImage(img)}
            />
          )}
        </>
      )}
    </div>
  );
}

function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <section className="section">
      <div className="section__head">
        <h2 className="section__title">{title}</h2>
        {hint && <span className="section__hint">{hint}</span>}
      </div>
      {children}
    </section>
  );
}

function BoardCard({
  dev,
  versions,
  channel,
  selected,
  onSelect,
}: {
  dev: CatalogDevice;
  versions: CatalogImage[];
  channel: Channel;
  selected: boolean;
  onSelect: () => void;
}) {
  const disabled = versions.length === 0;
  const tier = dev.tags.find((t) => ['community', 'legacy', 'wip', 'tv-box'].includes(t));
  return (
    <button
      className={`selectcard ${selected ? 'is-selected' : ''} ${disabled ? 'is-disabled' : ''}`}
      disabled={disabled}
      onClick={onSelect}
      title={disabled ? `No ${channel} image for this board yet` : undefined}
    >
      <span className="selectcard__check">
        <Check size={20} />
      </span>
      <BoardThumb id={dev.id} icon={dev.icon} />
      <span className="selectcard__body">
        <span className="selectcard__title">
          {dev.name}
          {dev.tags.includes('popular') && <span className="chip">Popular</span>}
        </span>
        <span className="selectcard__desc">{dev.description}</span>
        <span className="selectcard__meta">
          {versions.length > 0 ? (
            <>
              <span>latest v{versions[0].osVersion}</span>
              <span>·</span>
              <span>
                {versions.length} version{versions.length > 1 ? 's' : ''}
              </span>
            </>
          ) : (
            <>
              {dev.arch && <span>{dev.arch}</span>}
              {tier && (
                <>
                  {dev.arch && <span>·</span>}
                  <span>{tier}</span>
                </>
              )}
              <span>·</span>
              <span style={{ color: 'var(--muted)' }}>no {channel} image</span>
            </>
          )}
        </span>
      </span>
    </button>
  );
}

function LocalCard({
  name,
  size,
  comp,
  onClick,
}: {
  name: string;
  size: number;
  comp: string;
  onClick: () => void;
}) {
  return (
    <button className="selectcard is-selected" style={{ width: '100%', marginBottom: '1.1rem' }} onClick={onClick}>
      <span className="selectcard__check">
        <Check size={20} />
      </span>
      <span className="selectcard__icon">
        <FileUp size={22} />
      </span>
      <span className="selectcard__body">
        <span className="selectcard__title">Local image</span>
        <span className="selectcard__desc mono">{name}</span>
        <span className="selectcard__meta">
          <span>{formatBytes(size)}</span>
          <span>·</span>
          <span>{comp.toUpperCase()}</span>
          <span>·</span>
          <span>Click to choose another</span>
        </span>
      </span>
    </button>
  );
}

function Empty({ query, channel }: { query: string; channel: Channel }) {
  return (
    <div className="empty">
      <Search size={42} strokeWidth={1.2} />
      <div className="empty__title">No board matches “{query}”</div>
      <p>Try another name, switch channel (currently {channel}), or use a local image file.</p>
    </div>
  );
}

function VersionPanel({
  boardId,
  boardName,
  boardIcon,
  channel,
  versions,
  selected,
  onSelect,
}: {
  boardId: string;
  boardName: string;
  boardIcon: string;
  channel: Channel;
  versions: CatalogImage[];
  selected: CatalogImage;
  onSelect: (img: CatalogImage) => void;
}) {
  return (
    <div className="versionpanel">
      <div className="versionpanel__head">
        <span className="versionpanel__id">
          <BoardThumb id={boardId} icon={boardIcon} size="sm" />
          <span className="kicker">Selected — {boardName}</span>
        </span>
        <span className="chip">{channel}</span>
      </div>
      <div className="versionpanel__body">
        <div className="field" style={{ margin: 0, minWidth: 260 }}>
          <label className="field__label" htmlFor="version">
            Version
          </label>
          <Select
            id="version"
            ariaLabel="Version"
            value={selected.id}
            options={versions.map((v, i) => ({
              value: v.id,
              label: `v${v.osVersion}${v.codename ? ` (${v.codename})` : ''}${
                i === 0 ? ' — latest' : ''
              }`,
            }))}
            onChange={(id) => {
              const img = versions.find((v) => v.id === id);
              if (img) onSelect(img);
            }}
          />
        </div>
        <div className="versionpanel__meta mono">
          <span>{formatBytes(selected.downloadSize)} download</span>
          {selected.released && <span>· released {formatDate(selected.released)}</span>}
        </div>
      </div>
      {(selected.notes || selected.notesUrl) && (
        <div className="versionpanel__notes">
          {selected.notes && <p>{selected.notes}</p>}
          {selected.notesUrl && (
            <button className="linkbtn rowbtn" onClick={() => openExternal(selected.notesUrl!)}>
              What’s new in v{selected.osVersion} <ExternalLink size={12} />
            </button>
          )}
        </div>
      )}
    </div>
  );
}
