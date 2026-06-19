import { create } from 'zustand';
import {
  type AppSettings,
  type Catalog,
  type CatalogImage,
  type Customization,
  type DriveInfo,
  type FlashProgress,
  type ImageSource,
  type LocalImage,
  DEFAULT_CATALOG_URL,
  defaultCustomization,
} from './types';
import * as api from './lib/api';
import { isSupportedImage } from './lib/format';

/** Drop unsupported (legacy) images, then any device left with no images. */
function pruneCatalog(cat: Catalog): Catalog {
  const images = cat.images.filter((i) => isSupportedImage(i.osVersion, i.channel));
  const used = new Set(images.map((i) => i.device));
  const devices = cat.devices.filter((d) => used.has(d.id));
  return { ...cat, images, devices };
}

export type Step = 'image' | 'storage' | 'customize' | 'flash' | 'done';
export const STEPS: Step[] = ['image', 'storage', 'customize', 'flash', 'done'];

const SETTINGS_KEY = 'airwaves-flasher.settings';

function loadSettings(): AppSettings {
  const base: AppSettings = {
    catalogUrl: DEFAULT_CATALOG_URL,
    theme: 'dark',
    verifyAfterWrite: true,
    ejectWhenDone: true,
    cacheHours: 3,
  };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return { ...base, ...JSON.parse(raw) };
  } catch {
    /* ignore */
  }
  return base;
}

function persistSettings(s: AppSettings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  } catch {
    /* ignore */
  }
}

export function applyTheme(theme: 'dark' | 'light') {
  document.documentElement.setAttribute('data-theme', theme);
}

interface FlasherState {
  step: Step;
  settings: AppSettings;

  catalog: Catalog | null;
  catalogLoading: boolean;
  catalogError: string | null;

  source: ImageSource | null;
  drive: DriveInfo | null;
  customization: Customization;

  progress: FlashProgress | null;
  logs: string[];
  flashError: string | null;
  flashWarning: string | null;

  // actions
  init: () => void;
  goto: (s: Step) => void;
  next: () => void;
  back: () => void;

  loadCatalog: (force?: boolean) => Promise<void>;
  chooseImage: (image: CatalogImage) => void;
  chooseLocal: (file: LocalImage) => void;
  clearSource: () => void;
  chooseDrive: (d: DriveInfo | null) => void;
  setCustomization: (patch: Partial<Customization>) => void;
  updateSettings: (patch: Partial<AppSettings>) => void;

  beginFlash: () => Promise<void>;
  cancelFlash: () => Promise<void>;
  reset: () => void;
}

let unsubProgress: (() => void) | null = null;
let unsubLog: (() => void) | null = null;

export const useFlasher = create<FlasherState>((set, get) => ({
  step: 'image',
  settings: loadSettings(),
  catalog: null,
  catalogLoading: false,
  catalogError: null,
  source: null,
  drive: null,
  customization: defaultCustomization(),
  progress: null,
  logs: [],
  flashError: null,
  flashWarning: null,

  init: () => {
    applyTheme(get().settings.theme);
    void get().loadCatalog();
  },

  goto: (s) => set({ step: s }),

  next: () => {
    const { step } = get();
    const i = STEPS.indexOf(step);
    if (i < STEPS.length - 1) set({ step: STEPS[i + 1] });
  },

  back: () => {
    const { step } = get();
    const i = STEPS.indexOf(step);
    if (i > 0) set({ step: STEPS[i - 1] });
  },

  loadCatalog: async (force = false) => {
    if (get().catalogLoading) return;
    if (get().catalog && !force) return;
    set({ catalogLoading: true, catalogError: null });
    try {
      const cat = await api.fetchCatalog(get().settings.catalogUrl);
      set({ catalog: pruneCatalog(cat), catalogLoading: false });
    } catch (e) {
      set({
        catalogError: e instanceof Error ? e.message : String(e),
        catalogLoading: false,
      });
    }
  },

  chooseImage: (image) => {
    // default the OTA update channel to match the image's channel
    set((st) => ({
      source: { kind: 'catalog', image },
      customization: { ...st.customization, channel: st.customization.channel || image.channel },
    }));
  },

  chooseLocal: (file) => set({ source: { kind: 'local', file } }),

  clearSource: () => set({ source: null }),

  chooseDrive: (d) => set({ drive: d }),

  setCustomization: (patch) =>
    set((st) => ({ customization: { ...st.customization, ...patch } })),

  updateSettings: (patch) =>
    set((st) => {
      const settings = { ...st.settings, ...patch };
      persistSettings(settings);
      if (patch.theme) applyTheme(patch.theme);
      return { settings };
    }),

  beginFlash: async () => {
    const { source, drive, customization, settings } = get();
    if (!source || !drive) return;
    set({ step: 'flash', flashError: null, flashWarning: null, logs: [], progress: { phase: 'preparing', fraction: 0, bytesProcessed: 0, bytesTotal: 0, speedBps: 0, etaSecs: 0, message: 'Preparing…' } });

    unsubProgress?.();
    unsubLog?.();
    unsubProgress = api.onProgress((p) => {
      set({ progress: p });
      if (p.warning) {
        set((st) => ({ flashWarning: p.warning!, logs: [...st.logs.slice(-400), `Warning: ${p.warning}`] }));
      }
      if (p.phase === 'done') set({ step: 'done' });
      if (p.phase === 'error') set({ flashError: p.message, step: 'done' });
    });
    unsubLog = api.onLog((line) =>
      set((st) => ({ logs: [...st.logs.slice(-400), line] })),
    );

    try {
      await api.startFlash({
        source,
        device: drive,
        customization,
        verify: settings.verifyAfterWrite,
        eject: settings.ejectWhenDone,
        cacheSeconds: Math.max(0, Math.round((settings.cacheHours || 0) * 3600)),
      });
    } catch (e) {
      set({ flashError: e instanceof Error ? e.message : String(e), step: 'done' });
    }
  },

  cancelFlash: async () => {
    await api.cancelFlash();
  },

  reset: () => {
    unsubProgress?.();
    unsubLog?.();
    unsubProgress = null;
    unsubLog = null;
    set({
      step: 'image',
      source: null,
      drive: null,
      customization: defaultCustomization(),
      progress: null,
      logs: [],
      flashError: null,
      flashWarning: null,
    });
  },
}));
