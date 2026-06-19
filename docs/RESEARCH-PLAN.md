I now have full context. The frontend contract layer (`types.ts`, `store.ts`, `lib/api.ts`, `mockBackend.ts`, `theme.css`) is already designed and is the de-facto source of truth — the plan must align to it. `App.tsx` and the Rust backend are still the default scaffold. The design system is the actual Airwaves OS "night-watch" aesthetic (ink navy, amber beacon), which is more specific and better than the generic Airframes tokens in the research. I'll write the plan to honor the existing contract and fill in everything that's still scaffold.

Here is the plan.

---

# Airwaves OS Flasher — Implementation Plan

A cross-platform (macOS, Linux, Windows) image flasher for **Airwaves OS**, the Airframes aviation-data feeder OS. Built with **Tauri 2.x + Rust backend + React 19 frontend**. Original code, architecturally informed by rpi-imager, balenaEtcher, and caligula.

> **Grounding note.** The repo is already scaffolded with the chosen stack and — importantly — a fully-designed **frontend contract** (`src/types.ts`, `src/store.ts`, `src/lib/api.ts`, `src/lib/mockBackend.ts`) and a complete **design system** (`src/styles/theme.css`, the real Airwaves OS "night-watch" aesthetic). That contract is the source of truth this plan builds against; deviations below are called out explicitly. The Rust backend (`src-tauri/src/lib.rs`) and `App.tsx` are still the default Tauri template and are where most net-new work lands.

---

## 1. High-level architecture

### 1.1 Process model: thin GUI + elevated one-shot helper

Three cooperating pieces, mirroring the etcher/caligula consensus (GUI stays unprivileged; only the device-touching code is elevated):

```
┌─────────────────────────────────────────────────────────────┐
│  airwaves-flasher (Tauri, UNPRIVILEGED)                       │
│  ┌──────────────┐   invoke / Channel    ┌──────────────────┐ │
│  │ React 19 UI  │ ◄───────────────────► │ Rust commands    │ │
│  │ Zustand store│   flash://progress     │ (lib.rs)         │ │
│  └──────────────┘                        │ - fetch_catalog  │ │
│                                          │ - list_drives    │ │
│                                          │ - inspect_image  │ │
│                                          │ - detect_ssh_keys│ │
│                                          │ - start_flash ───┼─┐│
│                                          │ - cancel_flash   │ ││
│                                          └──────────────────┘ ││
└──────────────────────────────────────────────────────────────┘│
                                                  spawn elevated  │
                                          ┌───────────────────────▼─┐
                                          │ awflash-helper (ELEVATED)│
                                          │ standalone Rust CLI      │
                                          │ open→unmount/lock→write  │
                                          │ →flush→read-back verify  │
                                          │ →FAT customize           │
                                          │ emits NDJSON on stdout   │
                                          └──────────────────────────┘
```

- **GUI (unprivileged):** enumeration, catalog, UX, the authoritative **flash state machine**, and orchestration. Never runs as root/admin. Note: the existing `api.ts` listens for `flash://progress` / `flash://log` **events** — see §4.5 for the deliberate decision to keep the event-bus contract the scaffold already wired, with channels as an internal optimization.
- **awflash-helper (elevated):** a separate Rust binary that does the dangerous work. It re-validates the target device is removable+non-system **inside the elevated process** (never trusts the GUI), then opens → unmounts/locks → writes → flushes → reads-back → verifies → applies FAT customization → ejects. Streams newline-delimited JSON progress on stdout, exit code = result. This isolates all `unsafe`/privileged code into one small auditable binary and keeps the updater/auto-update from forcing elevation on the whole app.

**Why a helper and not "elevate the app":** elevating the whole app breaks `tauri-plugin-updater`, forces admin for trivial actions, and enlarges the privileged attack surface. The helper takes a device path as an argument and is the single LPE-sensitive surface — it gets the device re-validation guard.

### 1.2 Crate stack (Rust)

A workspace with a shared `awflash-core` library consumed by both the Tauri app and the helper:

| Concern | Crate | Notes |
|---|---|---|
| Drive enumeration | **`rs-drivelist`** (vendored/forked) | gives `isRemovable`/`isSystem`/`raw`/`busType`/`size`. Lightly maintained + winapi-based → **vendor it** into `crates/awflash-drivelist`. |
| HTTP download | **`reqwest`** (rustls, streaming) | streaming body → decompressor. `rustls` to avoid OpenSSL build pain. |
| Decompression — xz | **`liblzma`** `features=["static"]` | maintained xz2 fork; `static` vendors audited pre-backdoor liblzma (CVE-2024-3094 mitigation). `read::XzDecoder`. |
| Decompression — gz | **`flate2`** `features=["zlib-ng"]` | **`MultiGzDecoder`** (not `GzDecoder`). |
| Decompression — zst | **`zstd`** `0.13` | `stream::read::Decoder`; match `.zst` **and** `.zstd`. |
| Decompression — zip | **`zip`** `2.x` | needs `Read+Seek` ⇒ zip must be fully on disk first (see §4.2). |
| Hashing | **`sha2`** `0.10` | runtime SHA-NI backend; **pin hashing path to opt-level=3**. |
| FAT in-place edit | **`fatfs`** + **`gpt`/`mbrman`** | parse partition table, mount boot partition over the block device, write config files. |
| Raw device I/O | **`nix`** (Unix) + **`windows`/`windows-sys`** (Win32) | `O_DIRECT`/`O_EXCL`/`fsync` (Linux), `fcntl(F_NOCACHE)` (macOS), `CreateFileW`/`FSCTL_*`/`FlushFileBuffers` (Windows). |
| Aligned buffers | **`aligned-vec`** | 4096-byte aligned ~4 MiB write buffers. |
| Disk arbitration (macOS unmount/eject) | **`core-foundation`** + raw `DiskArbitration` FFI | `DADiskUnmount(kDADiskUnmountOptionWhole\|Force)`, `DADiskEject`. |
| Async runtime | **`tokio`** | download on async; write/verify on `spawn_blocking`. |
| Serde | **`serde`/`serde_json`** | catalog + IPC payloads (already in `Cargo.toml`). |
| Error | **`thiserror`** (lib) + typed error enum | classified errors → human messages (§5.3). |

### 1.3 Privilege escalation per OS

Centralized in `awflash-core::escalation` with `#[cfg(...)]` arms. The GUI command `start_flash` builds the helper invocation; the helper is launched **elevated**:

- **macOS:** the GUI calls the helper as a Tauri **sidecar**, but to elevate it runs `osascript -e 'do shell script "<helper> --device … " with administrator privileges'` (supports Touch ID, primes the sudo timestamp). The helper, once root, opens `/dev/rdiskN`. *Future-proofing:* a `SMAppService` LaunchDaemon + XPC helper is the Apple-blessed long-term path; deferred (notarization/signing complexity) but the helper boundary makes the swap localized.
- **Linux:** launch the helper via **`pkexec`** (polkit GUI prompt) with a shipped `org.airframes.awflash.policy`. Fallback chain: `pkexec` → `sudo` → `doas`. AppImage/headless path documents `sudo awflash-helper`.
- **Windows:** ship `awflash-helper.exe` with an embedded **`requireAdministrator`** manifest (via `embed-resource` in the helper's `build.rs`), launch with `ShellExecuteW` verb `"runas"` (triggers UAC). The **main app stays `asInvoker`** so the updater isn't broken.

Privilege-denied/cancelled is a first-class, recoverable error state (§5.3), not a crash.

### 1.4 Frontend stack

Already chosen and partly built — **keep it**:
- **React 19 + Zustand 5** (`src/store.ts` is the wizard state machine mirror).
- **Vite 7 + TypeScript 5.8.**
- **framer-motion 12** (progress/phase transitions, reduced-motion aware).
- **lucide-react** (icon set).
- **@fontsource** Archivo Variable / Instrument Serif / IBM Plex Mono (bundled, offline).
- `@tauri-apps/plugin-dialog` (image file picker), `@tauri-apps/plugin-opener` (external links).

Add: `@tauri-apps/plugin-store` (persist prefs Rust-side), `@tauri-apps/plugin-updater` + `@tauri-apps/plugin-process` (auto-update), `@tauri-apps/plugin-window-state`.

---

## 2. Project structure

Convert `src-tauri` into a Cargo **workspace** so the helper and core lib are siblings of the Tauri crate.

```
airwaves-os-flasher/
├─ index.html
├─ package.json            # add updater/store/window-state/process plugins
├─ vite.config.ts
├─ public/
│   └─ airwaves.png
├─ src/                                   # ── frontend (React) ──
│   ├─ main.tsx
│   ├─ App.tsx                            # REWRITE: wizard shell (currently template)
│   ├─ types.ts                           # ✅ exists — the UI⇄Rust contract
│   ├─ store.ts                           # ✅ exists — Zustand state machine
│   ├─ styles/
│   │   ├─ theme.css                      # ✅ exists — Airwaves "night-watch" tokens
│   │   └─ app.css                        # NEW: layout/shell/wizard CSS
│   ├─ lib/
│   │   ├─ api.ts                         # ✅ exists — native|mock bridge
│   │   ├─ mockBackend.ts                 # ✅ exists — browser-dev fallback
│   │   └─ format.ts                      # ✅ exists — bytes/speed/eta humanizers
│   ├─ components/                        # NEW
│   │   ├─ TitleBar.tsx                   # frameless chrome + traffic-light inset
│   │   ├─ Stepper.tsx                    # 5-step gated progress rail
│   │   ├─ DeviceCard.tsx                 # large tappable storage card
│   │   ├─ ImageCard.tsx                  # catalog entry card
│   │   ├─ ConfirmEraseDialog.tsx         # destructive confirm
│   │   ├─ ProgressArc.tsx                # determinate write/verify arc
│   │   └─ ui/ (Button, Field, Toggle, Chip, Modal, …)
│   ├─ screens/                           # NEW — one per wizard step
│   │   ├─ ChooseImage.tsx
│   │   ├─ ChooseStorage.tsx
│   │   ├─ Customize.tsx
│   │   ├─ Flash.tsx
│   │   └─ Done.tsx
│   ├─ i18n/                              # NEW — react-i18next, en first
│   └─ assets/brand/
├─ src-tauri/                             # ── Tauri app crate ──
│   ├─ Cargo.toml                         # workspace root or member
│   ├─ tauri.conf.json                    # window/bundle/updater/sidecar
│   ├─ build.rs
│   ├─ capabilities/
│   │   ├─ default.json                   # ✅ exists; add window/shell/updater perms
│   │   └─ flash.json                     # NEW: scope start_flash/list_drives + sidecar
│   ├─ src/
│   │   ├─ main.rs
│   │   ├─ lib.rs                          # REWRITE: register real commands
│   │   ├─ commands.rs                     # fetch_catalog/list_drives/inspect_image/…
│   │   ├─ catalog.rs                      # serde model + fetch + GitHub fallback
│   │   ├─ flash_orchestrator.rs           # spawns helper, parses NDJSON → events
│   │   └─ state.rs                         # Mutex<FlashState> + cancel token
│   └─ icons/
└─ crates/                                # ── shared Rust ──
    ├─ awflash-core/        # device I/O, decompress, hash, verify, FAT, escalation
    │   └─ src/{lib,source,decompress,detect,writer,verify,device/{macos,linux,windows},fat,escalation,progress,error}.rs
    ├─ awflash-drivelist/   # vendored/forked rs-drivelist
    └─ awflash-helper/      # elevated CLI bin: clap args + NDJSON stdout + build.rs(win manifest)
```

`tauri.conf.json` gets `bundle.externalBin: ["binaries/awflash-helper"]` (target-triple-suffixed at package time).

---

## 3. Image-catalog manifest schema

**Original schema** (not rpi-imager's `os_list`), and it must match the **already-defined `src/types.ts`** `Catalog`/`CatalogDevice`/`CatalogImage` interfaces — those are the contract. Flat arrays (devices, images) keyed by id, rather than rpi-imager's recursive tree, because Airwaves has a small fixed board matrix; the UI joins `image.device → device.id`.

The Rust serde model (`catalog.rs`) mirrors `types.ts` exactly (camelCase via `#[serde(rename_all = "camelCase")]`).

```jsonc
{
  "schema": 1,
  "name": "Airwaves OS",
  "updated": "2026-06-15T00:00:00Z",
  "channels": ["stable", "beta", "dev"],

  "devices": [
    {
      "id": "rpi4b",
      "name": "Raspberry Pi 3 / 4 / 5",
      "description": "64-bit ARM. The default feeder board.",
      "icon": "rpi",
      "arch": "arm64",
      "tags": ["rpi", "arm64", "default"]
    },
    {
      "id": "x86",
      "name": "PC / x86-64",
      "description": "Any amd64 machine — boots from USB.",
      "icon": "x86",
      "arch": "amd64",
      "tags": ["pc", "amd64"]
    }
  ],

  "images": [
    {
      "id": "rpi4b-stable-2026.06.1",
      "device": "rpi4b",
      "channel": "stable",
      "osVersion": "2026.06.1",
      "codename": "Approach",
      "name": "Airwaves OS 2026.06.1 (Raspberry Pi)",
      "released": "2026-06-12",
      "notes": "RTL-SDR auto-detect; HFDL profile defaults; faster first boot.",
      "notesUrl": "https://github.com/airframesio/airwaves-os/releases/tag/v2026.06.1",
      "url": "https://downloads.airframes.io/airwaves-os/2026.06.1/airwaves-os-rpi4b-2026.06.1.img.xz",
      "compression": "xz",
      "downloadSize": 412366848,
      "extractSize": 2576980377,
      "downloadSha256": "9f1c…",   // verifies the compressed download (fail fast)
      "sha256": "4ca8…",            // verifies the UNCOMPRESSED image (read-back target)
      "minStorageBytes": 4294967296,
      "latest": true
    }
  ],

  "source": "remote"
}
```

**Field rationale (vs rpi-imager):**
- `extractSize` is the **progress denominator** and the **card-capacity precheck** (`drive.sizeBytes >= image.extractSize`).
- Two hashes, two purposes: `downloadSha256` (cheap, catches network corruption before decompressing) and `sha256` (the canonical read-back verify target). Both optional so unsigned dev images still flash.
- `channel`/`latest`/`codename`/`notes` drive the UX (catalog filtering, "Use latest" affordance, release-notes link).
- `compression` is explicit so the UI labels it; the helper still **sniffs magic bytes** as ground truth (§4.2) and treats `compression` as a hint only.

**Sourcing & fallback** (already encoded in `types.ts`):
1. `DEFAULT_CATALOG_URL` = `…/airwaves-os/main/flasher/catalog.json` (raw GitHub).
2. If absent/unreachable, `fetch_catalog` falls back to the **GitHub Releases API** for `airframesio/airwaves-os`, synthesizing `CatalogImage`s from release assets (regex match `airwaves-os-<device>-<ver>.img.(xz|gz|zst)`), with `source: "github"`.
3. `"Use my own image"` is always available regardless of catalog state.

---

## 4. Core write pipeline

The canonical model: **download/open → decompress-on-the-fly → write raw device → flush → read-back verify**, never staging a decompressed temp file. Lives in `awflash-core`; executed inside `awflash-helper`.

### 4.1 Source acquisition

- **Catalog/remote:** `reqwest` streaming GET. Optionally verify `downloadSha256` of the compressed stream while downloading (fail fast). For non-zip formats the body pipes straight into the decompressor. For **zip**, the body is written to a temp file first (zip needs `Read+Seek`).
- **Local file:** opened directly; `inspect_image` (a separate cheap command) already sniffs size + compression for the picker.
- The GUI's `download` phase only applies to remote sources; local sources jump straight to `writing`. (`FlashPhase` in `types.ts` already enumerates `preparing/downloading/writing/verifying/configuring/finalizing/done`.)

### 4.2 Format detection + decompression

Single abstraction `open_image(reader) -> Box<dyn Read>` yielding **raw image bytes**:

1. **Sniff first 6 bytes** (magic): `FD 37 7A 58 5A 00`→xz, `1F 8B`→gz, `28 B5 2F FD`→zst, `50 4B 03 04`→zip; else treat as raw (`.img`/`.iso`).
2. Fall back to filename extension only if magic is ambiguous/absent. `compression` from the catalog is a hint, never authoritative.
3. Wrap:
   - xz → `liblzma::read::XzDecoder`
   - gz → `flate2::read::MultiGzDecoder` ⚠ **never `GzDecoder`** (truncates multistream → silent short write)
   - zst → `zstd::stream::read::Decoder`
   - zip → `ZipArchive::by_index` of the largest `*.img`/`*.iso` entry (from the temp file)
   - raw → the reader itself
- `.iso` and `.img` are written **byte-for-byte identically** (modern ISOs are isohybrid). No loop-mount, no extraction.

### 4.3 Raw device write

Per-platform open (`awflash-core::device`):
- **macOS:** rewrite `/dev/diskN`→`/dev/rdiskN`, `diskutil unmountDisk` (whole disk) first, open RW, `fcntl(fd, F_NOCACHE)`.
- **Linux:** open `/dev/sdX`/`/dev/mmcblkN` with `O_DIRECT | O_EXCL` (EXCL fails if a partition is mounted — a free safety lock); `umount` partitions first; `BLKDISCARD` (TRIM) + `BLKGETSIZE64` via `nix`.
- **Windows:** for each volume on the disk `CreateFileW` + `FSCTL_LOCK_VOLUME` + `FSCTL_DISMOUNT_VOLUME`, **keep those handles open for the whole write**; diskpart `clean` if partitions remain; open `\\.\PhysicalDriveN` with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`, `FILE_SHARE_READ|WRITE`.

Write loop (separate `spawn_blocking` task):
- 4 MiB **4096-aligned** buffer (`aligned-vec`); query real sector size, don't hardcode 512 vs 4096.
- Read from the decompressing reader → (a) update `bytesDecompressed` counter, (b) feed bytes into `Sha256` (the source/extract hash), (c) write to device, (d) update `bytesWritten`.
- **Zero-pad the final short block** to a full sector (unbuffered I/O can't do a short tail write).
- **Deferred-first-block trick:** hold back the first 4 KiB (boot sector/MBR), write the body, fsync, then apply FAT customization (§7), then write block 0 **last**, fsync again. An aborted flash never leaves a card that *looks* bootable but is corrupt — and it lets us customize the boot partition before the partition table is committed.
- Flush: `fsync`/`fdatasync` (Unix) / `FlushFileBuffers` (Windows).

### 4.4 Read-back verification

Two-hash model:
1. Source/extract hash = `Sha256` of decompressed bytes accumulated during write; compared to catalog `sha256`.
2. **Bust the cache** (Linux `posix_fadvise(POSIX_FADV_DONTNEED)`, or close+reopen via `/dev/rdiskN` on macOS / no-buffering handle on Windows), seek to 0, read back **exactly `extractSize` bytes** (the device is larger — don't hash trailing garbage), hash → the written/destination hash.
3. Pass requires both: source==catalog `sha256` (if published) **and** written==source. Mismatch → typed `VerifyMismatch` error, and the partition table (first 4 KiB) is zeroed so a bad card can't appear bootable.

⚠ Verification is the #1 false-negative UX bug class (Etcher's notorious "validation failed"). Mitigations: cache-bust before read-back, verify only to `extractSize`, handle short reads, and a *specific* non-alarming message on mismatch ("The card may be failing or counterfeit — try another card") rather than red FAILED.

### 4.5 Progress accounting + Tauri streaming

**Three independent counters** (`Arc<AtomicU64>`): downloaded (compressed), decompressed, written. User-facing bar = `written / extractSize` (the only reliably-known total). When `extractSize` is unknown, the write phase shows a byte counter, not a percent.

**Transport — deliberate decision:** the scaffold's `lib/api.ts` already subscribes to `flash://progress` and `flash://log` **events** with a typed `FlashProgress` payload. The research recommends `ipc::Channel` for high-frequency streams, but the event contract is already wired end-to-end (store → api → mock). **Keep the event-bus contract at the GUI boundary** (it's throttled to ≤10/sec so flooding isn't a concern), and use NDJSON-over-stdout from the helper internally. The orchestrator (`flash_orchestrator.rs`) reads the helper's stdout lines, maps them to `FlashProgress`, throttles, and `emit`s `flash://progress`. If profiling later shows event overhead, swapping the orchestrator→UI hop to a `Channel<FlashEvent>` is a localized change behind `api.ts`. Throttle: emit on ≥150 ms elapsed **or** ≥0.5% delta, whichever first.

Helper NDJSON line shape:
```jsonc
{"phase":"writing","bytesProcessed":123456789,"bytesTotal":2576980377,"speedBps":48234000,"etaSecs":51,"message":"Writing…"}
```
maps 1:1 to `FlashProgress`. The state machine lives in **Rust** (`state.rs`, `Mutex<FlashState>` + cancel token) so a webview reload can't orphan an in-flight write; the Zustand store is a mirror. Cancel = signal the cancel token → helper unmounts cleanly and exits non-zero with `cancelled`.

---

## 5. Drive enumeration + safety

### 5.1 Enumeration

`list_drives` → `awflash-drivelist::drive_list()` → `Vec<DriveInfo>` (matches `types.ts`). Internals are platform-native (lsblk JSON / `diskutil list -plist` / SetupAPI+IOCTLs). Populate `rawDevice` (macOS `/dev/rdiskN`), `system`, `removable`, `busType`, `mountpoints`, `readOnly`, `sizeBytes`.

Refresh on a ~1.5 s poll while the storage screen is mounted (optionally `nusb` hotplug watch later); diff against a path-keyed map so selection survives a refresh.

### 5.2 Filtering & display

- Default list = **`removable && !system && !readOnly`**.
- Internal/system disks hidden behind a friction toggle "Show internal disks", rendered in a danger style, requiring extra confirm.
- Each device shown as a **large card**: icon (SD/USB/SSD by `busType`), `description` (vendor+model), humanized `sizeBytes`, bus type — **never a bare path/drive-letter**.
- Capacity precheck: disable a card if `sizeBytes < image.extractSize` with an inline "Too small for this image".

### 5.3 Unmount / lock / confirm / errors

- **Before write:** unmount all volumes of the target (DiskArbitration `Whole|Force` / `umount` / `FSCTL_DISMOUNT_VOLUME`).
- **Final confirm dialog** names exact target + capacity: *"You are about to ERASE 'SanDisk Extreme 64 GB (/dev/disk4)'. All data will be lost."* No write begins without it. Consider a typed-confirm for system disks.
- **Re-validation in the elevated helper:** the helper independently re-checks the device is removable+non-system before opening — never trusts the GUI-supplied path. A boot-disk guard backstops `rs-drivelist`'s heuristic `isSystem`.
- **After write:** eject if `ejectWhenDone` (user-toggleable, default true).
- **Typed error classification** (`thiserror` → human copy, Retry preserves all selections):
  | Kind | Message / remediation |
  |---|---|
  | `PermissionDenied` | "Administrator access is required to write to disks." (re-prompt) |
  | `DeviceBusy` | "The drive is in use. Close apps using it and retry." |
  | `DeviceRemoved` | "The drive was disconnected during writing." |
  | `IoError` | "A write error occurred. The card may be failing." |
  | `InsufficientSpace` | "This image is larger than the selected drive." |
  | `VerifyMismatch` | "Verification failed — the card may be counterfeit/failing. Try another." |
  | `Cancelled` | (silent return to selections) |

---

## 6. UX flow + visual design

### 6.1 The flow (already encoded in `store.ts`: `image → storage → customize → flash → done`)

Gated five-step wizard; downstream steps dimmed/disabled until the prior decision is valid; one dominant CTA per screen.

**Step 1 — Choose Image** (`ChooseImage.tsx`)
- Network-fetched Airwaves catalog as selectable **image cards** (name, version, channel chip, codename, size, release date, "Latest" badge, notes link).
- Channel filter (stable/beta/dev). Device cards if multiple boards.
- First-class **"Use my own image"** (`pickLocalImage` → dialog → `inspect_image`).
- Shows expected `sha256` for trust. Catalog loading/error/retry states (already in store).

**Step 2 — Choose Storage** (`ChooseStorage.tsx`)
- Removable device cards, live-refreshing. "Show internal disks" friction toggle. Capacity gating. Empty state: "Insert a card or USB drive."

**Step 3 — Customize (optional, the differentiator)** (`Customize.tsx`)
- Maps 1:1 to `Customization` in `types.ts`: `hostname`, `wifiEnabled`+`wifi{ssid,psk}`, `channel` (OTA, defaults to image's channel), `timezone`, `sshEnabled`+`sshKeys[]` (with `detect_ssh_keys` autofill).
- **Skip** is a first-class button; `enabled` flag gates whether anything is written. "Save as defaults" persists via `plugin-store`.
- *Note:* station identity (operator/lat-long) is intentionally **out of scope** here — Airwaves OS configures that from its web dashboard on first boot (already documented in the `Customization` type comment). The flasher seeds connectivity + identity only.

**Step 4 — Flash** (`Flash.tsx`)
- Final **ERASE confirm dialog** → then two distinct labeled phases: **Writing** then **Verifying**, each a determinate `ProgressArc` with live MB/s + ETA. Collapsible log (`flash://log`). Cancel button wired to `cancelFlash`.

**Step 5 — Done** (`Done.tsx`)
- Calm success checkmark + auto-eject confirmation + "Flash another". On failure: classified message + **Retry as primary** (selections preserved). `store.reset()` returns to step 1.

### 6.2 Visual design language — **use the existing `theme.css`**

The scaffold's `theme.css` is the real **Airwaves OS "night-watch"** system and is richer/more on-brand than the generic Airframes web tokens in the research. Honor it:

- **Palette (dark default):** ink `#070a10`, panels `#0a0e16`/`#0c111c`, cream text `#ece5d6`, **amber beacon accent `#ffa14f`** (bright `#ffc372`, ember `#ff6a3d`), green `#59d98c`, red `#ff5c5c`, brand gradient `linear-gradient(115deg,#ffb347,#fc4a67,#8e54e9)`. Full **light theme** present.
- **Typography:** Archivo Variable (condensed, heavy headings `font-weight:740 font-stretch:118%`), Instrument Serif italic for emphasis, IBM Plex Mono for kickers/numbers/buttons (`tnum`).
- **Atmosphere:** vignette + film-grain overlays (`.atmosphere`), amber beacon glow bottom-right (`.glow`), sharp corners, uppercase mono buttons with `translateY(-2px)` hover + amber glow.
- **Accessibility:** danger/warning convey via **icon+text+color** (not color alone), WCAG-AA both themes, keyboard nav + visible focus, aria-labeled device cards, `aria-live` for phase changes, `prefers-reduced-motion` already handled in `theme.css`.

### 6.3 Window chrome

Frameless custom titlebar (`TitleBar.tsx`, `--titlebar-h:38px` already tokenized; `windowControls` already in `api.ts`). macOS: `titleBarStyle:"Overlay"` + `trafficLightPosition:{x:16,y:18}` (Tauri ≥2.4) to inset native lights into the custom bar. Win/Linux: custom min/max/close. `data-tauri-drag-region` on the bar. Add `tauri-plugin-window-state`. Capabilities need `core:window:default` + `core:window:allow-start-dragging` (+ minimize/maximize/close).

---

## 7. Pre-flash customization (boot-partition writing)

After the raw body lands but **before the deferred first block is committed**, the helper edits the boot partition **in place** (no OS mount):

1. Parse partition table: `gpt` (`EFI PART` @512) → fallback `mbrman` (`55 AA` @510). Locate the FAT boot partition.
2. Open it with **`fatfs`** layered over the block device (FAT16/FAT32 + VFAT LFN).
3. Write Airwaves config files (matching the `airwaves-os` / `airframes-client` conventions):
   - **`airwaves-install.json`** (the `AWCFG`-partition first-boot config consumed by `airwaves-preconfig`): `{ networking:{ hostname, wifi:{ssid,passphrase} }, channel, timezone, ssh:{ enabled, authorizedKeys:[…] } }`. Built from the `Customization` struct.
   - **WiFi:** pre-derive WPA2 PSK via **PBKDF2-HMAC-SHA1, 4096 iters, 32 bytes** — never write the plaintext passphrase.
   - **SSH:** write `authorized_keys` from `sshKeys[]`; toggle the SSH-enable flag. No password auth by default.
   - **Hostname:** if empty, leave the OS default (`airwaves-XXXXXX` via mDNS).
4. `dw.sync()` writes block 0 last; then the deferred first block + final fsync (§4.3).

No plaintext secrets ever hit the card (PSK pre-derived; no password storage — Airwaves OS uses default `airwaves/airwaves` + dashboard, not a baked password). All wrapped in try/catch → typed `CustomizeError`.

---

## 8. Build, signing, packaging, auto-update

### 8.1 Bundles
- **macOS:** signed + **notarized** `.dmg` + `.app` (Developer ID Application; App Store Connect API key in CI). Helper sidecar signed with the same identity, target-triple-suffixed.
- **Windows:** NSIS + MSI, **Authenticode via Azure Trusted Signing** (avoids SmartScreen "unknown publisher"). Helper exe carries the `requireAdministrator` manifest; main app `asInvoker`.
- **Linux:** **AppImage** + `.deb` (+ optional rpm). Ship the polkit `.policy`.

> Signing/notarization is **on the critical path**, not a finishing touch — an unsigned disk-writing tool triggers exactly the scary OS warnings that destroy trust. Stand up signing in CI early (Phase 5).

### 8.2 Sidecar wiring
`tauri.conf.json` → `bundle.externalBin: ["binaries/awflash-helper"]`; CI cross-builds the helper per target triple, renames with the `-<triple>[.exe]` suffix. Capability `flash.json` grants `shell:allow-execute` scoped to the sidecar (`sidecar:true, args:true`).

### 8.3 Auto-update
`tauri-plugin-updater`: `bundle.createUpdaterArtifacts:true`, `plugins.updater.pubkey` + templated `endpoints` → static `latest.json` on GitHub Releases (`{{target}}/{{arch}}/{{current_version}}`). `updater:default` capability. **Prompt-then-update** UX (version + notes + download channel), and **hard-gate any update check/install while a flash is in progress** (a relaunch mid-write corrupts cards).

### 8.4 CI
GitHub Actions matrix (macos-latest / ubuntu / windows). Build helper → build Tauri app → sign/notarize → publish artifacts + `latest.json`. Secrets: Apple API key, Azure signing creds, Tauri updater private key.

---

## 9. Phased build order + risks

### Phase 0 — Foundation (de-template)
- Rewrite `App.tsx` into the wizard shell rendering `screens/*` off `store.step`; build `TitleBar`, `Stepper`, `ui/*`; wire `theme.css` + `app.css`. Frontend runs **fully against `mockBackend.ts`** in `pnpm dev` (the bridge already supports this). **Outcome: clickable, on-brand, end-to-end UX with zero Rust** — fastest path to a demoable product and design-review target.

### Phase 1 — Catalog + enumeration (read-only Rust)
- Cargo workspace; `awflash-core` skeleton; vendor `rs-drivelist` → `awflash-drivelist`. Implement non-destructive commands: `fetch_catalog` (+GitHub fallback), `list_drives`, `inspect_image`, `detect_ssh_keys`, `eject_drive`. Replace mock for these. **No writing yet** — safe to run unsigned.

### Phase 2 — Write pipeline core (the dangerous part)
- `awflash-core`: `detect` → `decompress` (all 4 formats) → `writer` (per-OS open + aligned loop + deferred first block) → `verify` (read-back) → `progress`. Build `awflash-helper` CLI (NDJSON stdout). **Test against loopback devices / cheap USB sticks, including interrupted-write and counterfeit-card scenarios** before touching real workflows.

### Phase 3 — Orchestration + escalation
- `flash_orchestrator.rs` spawns helper, parses NDJSON → `flash://progress`/`flash://log`; `state.rs` cancel token. Per-OS escalation (osascript / pkexec / runas). End-to-end flash from the UI on all three platforms.

### Phase 4 — Customization
- FAT in-place editor; `airwaves-install.json` + WiFi PSK derivation + SSH keys + hostname, committed before the deferred first block. Validate against real Airwaves OS images on real hardware (first boot, mDNS, WiFi join).

### Phase 5 — Distribution
- Signing/notarization in CI, sidecar packaging, auto-update (`latest.json`), window-state + store plugins. i18n scaffolding (en).

### Phase 6 — Polish
- Hotplug watch, bottleneck detection (net/decompress/write-bound), reduced-motion celebration, full a11y pass, error-copy review, `/qa` + `/design-review`.

### Key risks & mitigations

| Risk | Mitigation |
|---|---|
| **Verification false-negatives** (top UX bug class) | Cache-bust before read-back; verify only to `extractSize`; handle short reads; specific calm messaging; gate success on confirmed flush. |
| **Deferred-first-block ordering bug → corrupt cards** | Port carefully; dedicated interrupted-write tests in Phase 2; fsync placement reviewed. |
| **`rs-drivelist` mis-classifies system disks** (USB-attached system, NVMe) | Vendor it; **independent boot-disk re-validation inside the elevated helper**; never write a `system` device without explicit override. |
| **Privilege prompts cancelled / MDM-blocked** | Treat denial as first-class recoverable `PermissionDenied`; Retry preserves state; per-OS fallback chain on Linux. |
| **Elevated helper = LPE surface** | Helper validates the device path is a real removable block device itself; minimal args; signed binary. |
| **xz supply chain (CVE-2024-3094)** | `liblzma` with `static` (vendored audited liblzma); never link system lib. |
| **`GzDecoder` truncation** | Lint/code-review rule: only `MultiGzDecoder`. |
| **sha2 slow under size-opt** | Pin release profile `opt-level=3` for the hashing/flashing crates. |
| **Zip streaming assumption** | Architecturally require full download to temp for zip; documented in `source`. |
| **State only in webview → orphaned write** | Authoritative state machine + cancel token in Rust; Zustand is a mirror. |
| **Auto-update mid-flash** | Hard-gate update check/install while `phase ∉ {idle,done,error}`. |
| **Signing lead time** | Start CI signing in Phase 5, but provision Apple/Azure accounts at project start. |
| **Custom chrome cross-OS jank** | macOS Overlay+traffic-lights vs Win/Linux custom buttons; test all three; consider `tauri-plugin-decorum`. |

---

**Relevant files (absolute):**
- Contract & state: `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/types.ts`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/store.ts`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/lib/api.ts`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/lib/mockBackend.ts`
- Design system: `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/styles/theme.css`
- To rewrite: `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src/App.tsx`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src-tauri/src/lib.rs`
- Config: `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src-tauri/Cargo.toml`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src-tauri/tauri.conf.json`, `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/src-tauri/capabilities/default.json`
- New Rust workspace: `/Users/kevin/Cloud/Dropbox/work/airframes/airwaves-os-flasher/crates/` (awflash-core, awflash-drivelist, awflash-helper)