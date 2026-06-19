/** Human-friendly formatting helpers. */

export function formatBytes(n: number, digits = 1): string {
  if (!n || n < 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  const v = n / Math.pow(1024, i);
  return `${v.toFixed(i === 0 ? 0 : digits)} ${units[i]}`;
}

export function formatSpeed(bps: number): string {
  if (!bps || bps <= 0) return '—';
  return `${formatBytes(bps)}/s`;
}

export function formatDuration(secs: number): string {
  if (!isFinite(secs) || secs <= 0) return '—';
  const s = Math.round(secs);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m < 60) return r ? `${m}m ${r}s` : `${m}m`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function formatDate(iso?: string): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

/** Curated IANA timezone list, falling back to the full Intl set when present. */
export function timezones(): string[] {
  const intl = Intl as unknown as { supportedValuesOf?: (k: string) => string[] };
  if (typeof intl.supportedValuesOf === 'function') {
    try {
      return intl.supportedValuesOf('timeZone');
    } catch {
      /* fall through */
    }
  }
  return [
    'UTC',
    'America/New_York',
    'America/Chicago',
    'America/Denver',
    'America/Los_Angeles',
    'America/Sao_Paulo',
    'Europe/London',
    'Europe/Paris',
    'Europe/Berlin',
    'Europe/Moscow',
    'Asia/Dubai',
    'Asia/Kolkata',
    'Asia/Singapore',
    'Asia/Tokyo',
    'Australia/Sydney',
    'Pacific/Auckland',
  ];
}

/** The first supported Airwaves OS release. Older builds (incl. the legacy
 * Armbian-style 23.11.* / 24.* date versions) are hidden. */
export const MIN_OS_VERSION = '1.0.37';

export function parseVersion(v: string): [number, number, number] {
  const p = v
    .trim()
    .replace(/^v/i, '')
    .split('.')
    .map((n) => parseInt(n, 10) || 0);
  return [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0];
}

/** Newest first. */
export function compareVersions(a: string, b: string): number {
  const pa = parseVersion(a);
  const pb = parseVersion(b);
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

/** Legacy Armbian-era date version (23.11.*, 24.*). A major >= 20 is always
 * dropped, even though it sorts "higher" than 1.0.37. */
export function isLegacyVersion(v: string): boolean {
  return parseVersion(v)[0] >= 20;
}

/** Channel-aware support rule:
 *  - legacy date versions are always hidden;
 *  - stable/beta require >= MIN_OS_VERSION (1.0.37);
 *  - dev allows any current-era 1.x build (the dev line currently sits at 1.0.24). */
export function isSupportedImage(version: string, channel: string): boolean {
  if (isLegacyVersion(version)) return false;
  if (channel === 'dev') return true;
  return compareVersions(version, MIN_OS_VERSION) >= 0;
}

/** Truncate in the middle, keeping the start and end (e.g. the key comment). */
export function middleEllipsis(s: string, max = 64): string {
  if (s.length <= max) return s;
  const head = Math.ceil((max - 1) / 2);
  const tail = Math.floor((max - 1) / 2);
  return `${s.slice(0, head)}…${s.slice(s.length - tail)}`;
}

export function guessTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

/** Basic RFC-1123 hostname validation matching the OS preconfig rules. */
export function isValidHostname(name: string): boolean {
  if (!name) return false;
  if (name.length > 63) return false;
  return /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(name.toLowerCase());
}

export function isValidSshKey(key: string): boolean {
  return /^(ssh-(rsa|ed25519|dss)|ecdsa-sha2-\S+|sk-ssh-\S+|sk-ecdsa-\S+)\s+\S+/.test(
    key.trim(),
  );
}
