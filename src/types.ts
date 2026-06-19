/*
 * Shared domain model for the Airwaves OS Flasher.
 *
 * These types mirror the Rust structs in `src-tauri/src/model.rs` (serde
 * camelCase). They are the single contract between the UI and the backend.
 */

export type Channel = 'stable' | 'beta' | 'dev';

export type Compression = 'xz' | 'gz' | 'zst' | 'zip' | 'none';

/* ---- image catalog ------------------------------------------------------- */

/** A hardware target an image can be flashed to. */
export interface CatalogDevice {
  id: string;
  name: string;
  description: string;
  /** logical icon key resolved in the UI (e.g. "rpi", "rock", "orangepi", "x86") */
  icon: string;
  arch: string;
  /** manufacturer/brand group, for vendor-grouped browsing */
  vendor?: string;
  tags: string[];
}

/** A single flashable image. */
export interface CatalogImage {
  id: string;
  /** references CatalogDevice.id */
  device: string;
  channel: Channel;
  osVersion: string;
  codename?: string;
  name: string;
  released?: string;
  notes?: string;
  notesUrl?: string;
  /** download URL of the (compressed) image */
  url: string;
  compression: Compression;
  /** bytes of the compressed download (may be 0 if unknown) */
  downloadSize: number;
  /** bytes of the uncompressed image written to the device (0 if unknown) */
  extractSize: number;
  /** sha256 of the compressed download, if published (verifies the download) */
  downloadSha256?: string;
  /** sha256 of the uncompressed image, if published (verifies what's written) */
  sha256?: string;
  /** minimum storage device size required, in bytes */
  minStorageBytes?: number;
  /** true for the newest image of its device+channel */
  latest?: boolean;
}

export interface Catalog {
  schema: number;
  name: string;
  updated?: string;
  channels: Channel[];
  devices: CatalogDevice[];
  images: CatalogImage[];
  /** where this catalog came from, for UI provenance */
  source?: 'remote' | 'github' | 'bundled';
}

/* ---- selectable image source (catalog item or local file) ---------------- */

export interface LocalImage {
  path: string;
  name: string;
  size: number;
  compression: Compression;
  /** uncompressed size if we could determine it */
  extractSize?: number;
}

export type ImageSource =
  | { kind: 'catalog'; image: CatalogImage }
  | { kind: 'local'; file: LocalImage };

/* ---- storage devices ----------------------------------------------------- */

export interface DriveInfo {
  /** stable identifier (usually the device path) */
  id: string;
  /** OS device path: /dev/disk4, /dev/sdb, \\\\.\\PhysicalDrive2 */
  device: string;
  /** macOS raw character device for fast writes: /dev/rdisk4 */
  rawDevice?: string;
  /** human label: vendor + model */
  description: string;
  sizeBytes: number;
  removable: boolean;
  /** the OS/boot disk — never offered as a normal target */
  system: boolean;
  busType?: string;
  mountpoints: string[];
  readOnly: boolean;
}

/* ---- first-boot customization (writes AWCFG/airwaves-install.json) ------- */

export interface WifiConfig {
  ssid: string;
  psk: string;
}

/**
 * Maps 1:1 onto the fields `airwaves-preconfig` actually applies on first
 * boot. Station identity (operator/lat-long) is intentionally absent — the
 * OS configures that from the web dashboard, not from the AWCFG partition.
 */
export interface Customization {
  enabled: boolean;
  hostname: string;
  wifi: WifiConfig;
  wifiEnabled: boolean;
  /** OTA update channel; defaults to the selected image's channel */
  channel: Channel | '';
  timezone: string;
  sshEnabled: boolean;
  sshKeys: string[];
}

export function defaultCustomization(): Customization {
  return {
    enabled: false,
    hostname: '',
    wifi: { ssid: '', psk: '' },
    wifiEnabled: false,
    channel: '',
    timezone: '',
    sshEnabled: false,
    sshKeys: [],
  };
}

/* ---- flashing pipeline --------------------------------------------------- */

export type FlashPhase =
  | 'idle'
  | 'preparing'
  | 'downloading'
  | 'writing'
  | 'verifying'
  | 'configuring'
  | 'finalizing'
  | 'done'
  | 'error'
  | 'cancelled';

export interface FlashProgress {
  phase: FlashPhase;
  /** 0..1 within the current phase */
  fraction: number;
  bytesProcessed: number;
  bytesTotal: number;
  speedBps: number;
  etaSecs: number;
  /** true while throughput is still settling — show "Pending" not a wild ETA */
  etaPending?: boolean;
  /** non-fatal warning to surface on completion (e.g. config not written) */
  warning?: string;
  message: string;
}

export interface FlashRequest {
  source: ImageSource;
  device: DriveInfo;
  customization: Customization;
  /** verify the device read-back after writing */
  verify: boolean;
  /** eject/unmount the device when finished */
  eject: boolean;
  /** keep the downloaded image cached this many seconds for reuse (0 = off) */
  cacheSeconds: number;
}

export interface AppSettings {
  catalogUrl: string;
  theme: 'dark' | 'light';
  verifyAfterWrite: boolean;
  ejectWhenDone: boolean;
  /** how long to keep downloaded images cached, in hours (0 = off) */
  cacheHours: number;
}

export const DEFAULT_CATALOG_URL =
  'https://raw.githubusercontent.com/airframesio/airwaves-os/main/flasher/catalog.json';

/** owner/repo used for the GitHub Releases fallback when the catalog is absent */
export const GITHUB_REPO = 'airframesio/airwaves-os';
