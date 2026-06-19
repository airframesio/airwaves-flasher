/*
 * Backend bridge. Inside the packaged Tauri app these call real Rust commands
 * and listen to native progress events. In a plain browser (`pnpm dev`) they
 * transparently fall back to the mock so the UI is fully exercisable.
 */
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  Catalog,
  DriveInfo,
  FlashProgress,
  FlashRequest,
  LocalImage,
} from '../types';
import {
  mockCatalog,
  mockDrives,
  mockLocalImage,
  mockSshKeys,
  runMockFlash,
} from './mockBackend';

const NATIVE = isTauri();

let mockHandle: { cancel: () => void } | null = null;
const progressSubs = new Set<(p: FlashProgress) => void>();
const logSubs = new Set<(line: string) => void>();

/* ---- queries ------------------------------------------------------------- */

export async function listDrives(): Promise<DriveInfo[]> {
  if (!NATIVE) return new Promise((r) => setTimeout(() => r(mockDrives), 400));
  return invoke<DriveInfo[]>('list_drives');
}

export async function fetchCatalog(url: string): Promise<Catalog> {
  if (!NATIVE) return new Promise((r) => setTimeout(() => r(mockCatalog), 500));
  return invoke<Catalog>('fetch_catalog', { url });
}

export async function pickLocalImage(): Promise<LocalImage | null> {
  if (!NATIVE) return new Promise((r) => setTimeout(() => r(mockLocalImage), 200));
  const { open } = await import('@tauri-apps/plugin-dialog');
  const path = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: 'Disk images', extensions: ['img', 'iso', 'xz', 'gz', 'zst', 'zip'] },
      { name: 'All files', extensions: ['*'] },
    ],
  });
  if (!path || typeof path !== 'string') return null;
  return invoke<LocalImage>('inspect_image', { path });
}

export async function detectSshKeys(): Promise<string[]> {
  if (!NATIVE) return mockSshKeys;
  return invoke<string[]>('detect_ssh_keys');
}

export async function ejectDrive(device: string): Promise<void> {
  if (!NATIVE) return;
  await invoke('eject_drive', { device });
}

export async function openExternal(url: string): Promise<void> {
  if (!NATIVE) {
    window.open(url, '_blank');
    return;
  }
  const { openUrl } = await import('@tauri-apps/plugin-opener');
  await openUrl(url);
}

/* ---- flashing ------------------------------------------------------------ */

export async function startFlash(req: FlashRequest): Promise<void> {
  if (!NATIVE) {
    mockHandle = runMockFlash(
      req,
      (p) => progressSubs.forEach((cb) => cb(p)),
      (l) => logSubs.forEach((cb) => cb(l)),
    );
    return;
  }
  await invoke('start_flash', { request: req });
}

export async function cancelFlash(): Promise<void> {
  if (!NATIVE) {
    mockHandle?.cancel();
    return;
  }
  await invoke('cancel_flash');
}

export function onProgress(cb: (p: FlashProgress) => void): () => void {
  if (!NATIVE) {
    progressSubs.add(cb);
    return () => progressSubs.delete(cb);
  }
  const un = listen<FlashProgress>('flash://progress', (e) => cb(e.payload));
  return () => { void un.then((f) => f()); };
}

export function onLog(cb: (line: string) => void): () => void {
  if (!NATIVE) {
    logSubs.add(cb);
    return () => logSubs.delete(cb);
  }
  const un = listen<string>('flash://log', (e) => cb(e.payload));
  return () => { void un.then((f) => f()); };
}

/* ---- window controls (custom title bar) ---------------------------------- */

export const platform: 'macos' | 'windows' | 'linux' | 'web' = (() => {
  if (!NATIVE) return 'web';
  const ua = navigator.userAgent;
  if (/Mac/.test(ua)) return 'macos';
  if (/Win/.test(ua)) return 'windows';
  return 'linux';
})();

export const windowControls = {
  async minimize() {
    if (!NATIVE) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().minimize();
  },
  async toggleMaximize() {
    if (!NATIVE) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().toggleMaximize();
  },
  async close() {
    if (!NATIVE) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().close();
  },
};

export const isNative = NATIVE;
