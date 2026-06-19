import { Check } from 'lucide-react';
import { useFlasher, type Step } from '../store';
import { formatBytes } from '../lib/format';

const ORDER: Step[] = ['image', 'storage', 'customize', 'flash'];
const META: Record<string, { n: number; label: string; sub: string }> = {
  image: { n: 1, label: 'Image', sub: 'Choose your board' },
  storage: { n: 2, label: 'Storage', sub: 'Where to write' },
  customize: { n: 3, label: 'Customize', sub: 'First boot · optional' },
  flash: { n: 4, label: 'Write', sub: 'Write & verify' },
};

export function StepRail() {
  const { step, source, drive, goto } = useFlasher();
  const curIdx = step === 'done' ? 3 : ORDER.indexOf(step);
  const busy = step === 'flash' || step === 'done';

  const imageLabel =
    source?.kind === 'catalog'
      ? source.image.name
      : source?.kind === 'local'
        ? source.file.name
        : null;
  const versionLabel =
    source?.kind === 'catalog'
      ? `v${source.image.osVersion}${source.image.channel !== 'stable' ? ` · ${source.image.channel}` : ''}`
      : source?.kind === 'local'
        ? 'local file'
        : null;

  return (
    <aside className="rail">
      <ol className="rail__steps">
        {ORDER.map((s, i) => {
          const done = step === 'done' || i < curIdx;
          const active = i === curIdx && step !== 'done';
          const clickable = !busy && i < curIdx;
          return (
            <li
              key={s}
              className={`railstep ${active ? 'is-active' : ''} ${done ? 'is-done' : ''} ${
                clickable ? 'is-clickable' : ''
              }`}
              onClick={() => clickable && goto(s)}
            >
              <span className="railstep__num">{done ? <Check size={13} /> : META[s].n}</span>
              <span className="railstep__text">
                <strong>{META[s].label}</strong>
                <span>{META[s].sub}</span>
              </span>
            </li>
          );
        })}
      </ol>

      <div className="rail__summary">
        <div className="rail__summary-head">Selection</div>
        <SummaryRow label="Image" value={imageLabel} mono />
        <SummaryRow label="Version" value={versionLabel} />
        <SummaryRow label="Drive" value={drive?.description ?? null} />
        <SummaryRow
          label="Target"
          value={drive ? `${drive.device} · ${formatBytes(drive.sizeBytes)}` : null}
          mono
        />
      </div>
    </aside>
  );
}

function SummaryRow({ label, value, mono }: { label: string; value: string | null; mono?: boolean }) {
  return (
    <div className="summaryrow">
      <span className="summaryrow__label">{label}</span>
      <span className={`summaryrow__value ${mono ? 'mono' : ''} ${value ? '' : 'is-empty'}`}>
        {value ?? '—'}
      </span>
    </div>
  );
}
