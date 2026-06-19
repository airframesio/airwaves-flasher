/*
 * Browser-dev mock of the native backend. Active only when NOT running inside
 * Tauri (i.e. `pnpm dev` in a normal browser), so the UI and design can be
 * built and reviewed without the Rust side. Inside the packaged app the real
 * Tauri commands are used instead (see api.ts).
 */
import type { Catalog, DriveInfo, FlashProgress, FlashRequest, LocalImage } from '../types';
import { armbianDevices, airwavesImages } from '../data/armbianBoards';

export const mockCatalog: Catalog = {
  schema: 1,
  name: 'Airwaves OS',
  updated: '2026-06-17T00:00:00Z',
  source: 'remote',
  channels: ['stable', 'beta', 'dev'],
  devices: armbianDevices,
  images: airwavesImages,
};

export const mockDrives: DriveInfo[] = [
  {
    id: '/dev/disk6',
    device: '/dev/disk6',
    rawDevice: '/dev/rdisk6',
    description: 'SanDisk Extreme microSD',
    sizeBytes: 64_088_457_216,
    removable: true,
    system: false,
    busType: 'USB',
    mountpoints: ['/Volumes/UNTITLED'],
    readOnly: false,
  },
  {
    id: '/dev/disk7',
    device: '/dev/disk7',
    rawDevice: '/dev/rdisk7',
    description: 'Kingston DataTraveler USB',
    sizeBytes: 31_914_983_424,
    removable: true,
    system: false,
    busType: 'USB',
    mountpoints: [],
    readOnly: false,
  },
];

export const mockLocalImage: LocalImage = {
  path: '/Users/you/Downloads/Airwaves_OS_uefi-x86-1.0.37.img.xz',
  name: 'Airwaves_OS_uefi-x86-1.0.37.img.xz',
  size: 740_000_000,
  compression: 'xz',
  extractSize: 2_900_000_000,
};

export const mockSshKeys = [
  'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI...mockkey you@laptop',
];

/** Simulates a flash run, emitting progress to the supplied callback. */
export function runMockFlash(
  req: FlashRequest,
  emit: (p: FlashProgress) => void,
  log: (line: string) => void,
): { cancel: () => void } {
  let cancelled = false;
  const total =
    req.source.kind === 'catalog'
      ? req.source.image.extractSize
      : req.source.file.extractSize ?? req.source.file.size;

  const phases: Array<{ phase: FlashProgress['phase']; secs: number; msg: string }> = [
    { phase: 'preparing', secs: 1.2, msg: 'Unmounting target volume...' },
    ...(req.source.kind === 'catalog'
      ? [{ phase: 'downloading' as const, secs: 4, msg: 'Downloading image' }]
      : []),
    { phase: 'writing', secs: 6, msg: 'Writing image to device' },
    ...(req.verify ? [{ phase: 'verifying' as const, secs: 3, msg: 'Verifying written data' }] : []),
    ...(req.customization.enabled
      ? [{ phase: 'configuring' as const, secs: 0.8, msg: 'Writing first-boot configuration...' }]
      : []),
    { phase: 'finalizing', secs: 0.6, msg: 'Flushing and ejecting...' },
  ];

  let pi = 0;
  log(`Starting flash to ${req.device.device}`);

  const tick = () => {
    if (cancelled) return;
    const ph = phases[pi];
    let local = 0;
    const step = () => {
      if (cancelled) return;
      local += 0.04 + Math.random() * 0.05;
      const f = Math.min(local, 1);
      const bytes = Math.round(total * f);
      emit({
        phase: ph.phase,
        fraction: f,
        bytesProcessed: bytes,
        bytesTotal: total,
        speedBps: total / Math.max(ph.secs, 0.1),
        etaSecs: Math.max(0, ph.secs * (1 - f)),
        // mimic the real warmup: ETA settles after the first stretch of a phase
        etaPending: (ph.phase === 'writing' || ph.phase === 'downloading') && f < 0.18,
        message: ph.msg,
      });
      if (f >= 1) {
        pi += 1;
        if (pi >= phases.length) {
          emit({
            phase: 'done',
            fraction: 1,
            bytesProcessed: total,
            bytesTotal: total,
            speedBps: 0,
            etaSecs: 0,
            message: 'Airwaves OS is ready. Insert the card and power on.',
          });
          log('Flash complete.');
          return;
        }
        setTimeout(tick, 250);
      } else {
        setTimeout(step, (ph.secs * 1000) / 22);
      }
    };
    step();
  };
  setTimeout(tick, 300);

  return {
    cancel: () => {
      cancelled = true;
      emit({ phase: 'cancelled', fraction: 0, bytesProcessed: 0, bytesTotal: total, speedBps: 0, etaSecs: 0, message: 'Cancelled' });
    },
  };
}
