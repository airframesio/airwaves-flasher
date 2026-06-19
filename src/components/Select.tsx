import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Search } from 'lucide-react';

export interface SelectOption {
  value: string;
  label: string;
  hint?: string;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  /** show a filter input inside the panel (constrained to options, no free text) */
  searchable?: boolean;
  placeholder?: string;
  ariaLabel?: string;
  id?: string;
}

/**
 * Custom dropdown matching the app's squared, mono button styling. When
 * `searchable`, the panel shows a filter field but only listed options can be
 * chosen — no custom values.
 */
export function Select({
  value,
  options,
  onChange,
  searchable = false,
  placeholder = 'Select…',
  ariaLabel,
  id,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [dropUp, setDropUp] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const selected = options.find((o) => o.value === value) ?? null;

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter((o) => o.label.toLowerCase().includes(q));
  }, [options, query]);

  // close on outside click
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [open]);

  // decide drop direction + focus search on open
  useLayoutEffect(() => {
    if (!open) return;
    const rect = rootRef.current?.getBoundingClientRect();
    if (rect) setDropUp(window.innerHeight - rect.bottom < 280 && rect.top > 280);
    const i = Math.max(0, filtered.findIndex((o) => o.value === value));
    setActive(i);
    if (searchable) setTimeout(() => inputRef.current?.focus(), 0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // keep the active option scrolled into view
  useEffect(() => {
    if (!open) return;
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${active}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [active, open]);

  function commit(v: string) {
    onChange(v);
    setOpen(false);
    setQuery('');
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (!open && (e.key === 'Enter' || e.key === 'ArrowDown' || e.key === ' ')) {
      e.preventDefault();
      setOpen(true);
      return;
    }
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      setOpen(false);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((a) => Math.min(filtered.length - 1, a + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const opt = filtered[active];
      if (opt) commit(opt.value);
    }
  }

  return (
    <div className="dropdown" ref={rootRef}>
      <button
        type="button"
        id={id}
        className={`dropdown__trigger ${open ? 'is-open' : ''}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={onKeyDown}
      >
        <span className={`dropdown__value ${selected ? '' : 'is-placeholder'}`}>
          {selected ? selected.label : placeholder}
        </span>
        <ChevronDown size={15} className="dropdown__chev" />
      </button>

      {open && (
        <div className={`dropdown__panel ${dropUp ? 'is-up' : ''}`} role="listbox">
          {searchable && (
            <div className="dropdown__search">
              <Search size={14} />
              <input
                ref={inputRef}
                value={query}
                placeholder="Type to filter…"
                spellCheck={false}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setActive(0);
                }}
                onKeyDown={onKeyDown}
              />
            </div>
          )}
          <div className="dropdown__list" ref={listRef}>
            {filtered.length === 0 && <div className="dropdown__empty">No matches</div>}
            {filtered.map((o, i) => (
              <button
                type="button"
                key={o.value}
                data-idx={i}
                className={`dropdown__option ${o.value === value ? 'is-selected' : ''} ${
                  i === active ? 'is-active' : ''
                }`}
                role="option"
                aria-selected={o.value === value}
                onMouseEnter={() => setActive(i)}
                onClick={() => commit(o.value)}
              >
                <span className="dropdown__check">{o.value === value && <Check size={14} />}</span>
                <span className="dropdown__optlabel">{o.label}</span>
                {o.hint && <span className="dropdown__hint">{o.hint}</span>}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
