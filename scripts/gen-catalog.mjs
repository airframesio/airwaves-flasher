#!/usr/bin/env node
/*
 * Generates the Airwaves OS flasher catalog data from the Armbian board configs.
 *
 *   node scripts/gen-catalog.mjs [path-to-armbian-build]
 *
 * Reads <armbian>/config/boards/*.{conf,csc,wip,eos,tvb} and emits:
 *   - src/data/armbianBoards.ts   (armbianDevices + airwavesImages, used by the
 *                                  mock and merged into the live catalog's board list)
 *   - docs/catalog.example.json   (a complete, ready-to-publish catalog)
 *
 * Re-run whenever the Armbian board set or the Airwaves image set changes.
 */
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const armbian = resolve(repo, process.argv[2] ?? '../armbian-build');
const boardsDir = join(armbian, 'config', 'boards');

const TIER = { conf: 'supported', csc: 'community', wip: 'wip', eos: 'legacy', tvb: 'tv-box' };

// ---- parse Armbian board configs ------------------------------------------
function val(text, key) {
  const m = text.match(new RegExp(`${key}=(["'])(.*?)\\1`)) || text.match(new RegExp(`${key}=(\\S+)`));
  return m ? (m[2] ?? m[1]) : '';
}
function firstComment(text) {
  for (const line of text.split('\n')) {
    const t = line.trim();
    if (t.startsWith('#') && t.length > 2 && !/shellcheck|SPDX|maintainer/i.test(t)) {
      return t.replace(/^#+\s*/, '').trim();
    }
    if (t && !t.startsWith('#')) break;
  }
  return '';
}
function archFor(family, id, name) {
  const f = `${family} ${id} ${name}`.toLowerCase();
  if (/x86|uefi|amd64|generic/.test(f)) return 'amd64';
  if (/rk35|rk33|rk3399|3588|meson64|sun50|sunxi64|s905|bcm2711|bcm2712|odroidn|odroidm|a311|sm8|armada/.test(f)) return 'arm64';
  if (/sun[0-9]|sun8|meson(?!64)|imx6|odroidxu|bcm2709|s5p|exynos/.test(f)) return 'armhf';
  return '';
}
function iconFor(family, id, name) {
  const f = `${family} ${id} ${name}`.toLowerCase();
  if (/bcm271|raspberry|rpi/.test(f)) return 'rpi';
  if (/rockchip|rock-?5|rock-?pi|rk35|rk33|rk3399|3588/.test(f)) return 'rock';
  if (/orangepi|orange pi/.test(f)) return 'orangepi';
  if (/x86|uefi|amd64/.test(f)) return 'x86';
  return 'sbc';
}

// Derive a manufacturer/brand group from the board name+id. Heuristic — good for
// the common brands, falls back to the leading word of the board name.
function vendorFor(id, name) {
  const s = `${name} ${id}`.toLowerCase();
  const map = [
    [/raspberry|(^|[^a-z])rpi/, 'Raspberry Pi'],
    [/radxa|rock-?pi|rock-?\d|rock ?\d|rockpro|rock64/, 'Radxa'],
    [/orange ?pi|orangepi/, 'Orange Pi'],
    [/nanopi|nanopc|nano-?pi|zeropi/, 'FriendlyELEC'],
    [/odroid/, 'Hardkernel'],
    [/banana ?pi|bananapi|(^|[^a-z])bpi|lamobo/, 'Banana Pi'],
    [/khadas|(^|[^a-z])vim\d/, 'Khadas'],
    [/pine ?64|pinebook|sopine|quartz|star64|pinephone|pinetab|soquartz/, 'Pine64'],
    [/olimex|olinuxino|(^|[^a-z])lime/, 'Olimex'],
    [/tinker ?board|tinkerboard/, 'ASUS'],
    [/clearfog|macchiatobin|cubox|hummingboard/, 'SolidRun'],
    [/cubie/, 'Cubietech'],
    [/helios/, 'Kobol'],
    [/le-?potato|sweet-?potato|libre|roc-|renegade|tritium|aml-/, 'Libre Computer'],
    [/jetson|tegra/, 'NVIDIA'],
    [/beagle/, 'BeagleBoard'],
    [/visionfive|jh7110|jh7100|starfive/, 'StarFive'],
    [/hifive|sifive|unmatched|unleashed/, 'SiFive'],
    [/lichee|sipeed|longan|(^|[^a-z])nezha|maix/, 'Sipeed'],
    [/pcduino/, 'LinkSprite'],
    [/youyeetoo/, 'youyeetoo'],
    [/uefi|x86|amd64|generic|(^|[^a-z])pc([^a-z]|$)/, 'PC / x86'],
    [/x96|h96|mxq|tx3|t95|magicsee|tvbox|tv-?box|station ?m|(^|[^a-z])(z28|xt-q8|mk808|rk322x|rk3318|rk3328|onecloud)/, 'TV boxes'],
  ];
  for (const [re, v] of map) if (re.test(s)) return v;
  const first = (name.split(/\s+/)[0] || 'Other').replace(/[^A-Za-z0-9].*$/, '');
  return canonicalVendor(first ? first[0].toUpperCase() + first.slice(1) : 'Other');
}

// Collapse casing variants and product/sub-brand names to one canonical vendor,
// so the manufacturer grid has no duplicates (e.g. FriendlyElec/FE -> FriendlyELEC,
// Station -> Firefly, Rock -> Radxa, ThinkPad -> Lenovo).
const VENDOR_ALIAS = {
  friendlyelec: 'FriendlyELEC', fe: 'FriendlyELEC', zeropi: 'FriendlyELEC',
  hinlink: 'Hinlink',
  armsom: 'ArmSoM',
  pine: 'Pine64',
  youyeetoo: 'youyeetoo',
  rock: 'Radxa',
  station: 'Firefly', core3566: 'Firefly',
  tinker: 'ASUS',
  la: 'Libre Computer', lamobo: 'Banana Pi', sweet: 'Libre Computer',
  visionfive: 'StarFive', visionfive2: 'StarFive',
  unleashed: 'SiFive', unmatched: 'SiFive',
  licheepi: 'Sipeed', longan: 'Sipeed', longanpi: 'Sipeed', nezha: 'Sipeed',
  thinkpad: 'Lenovo',
  pcduino: 'LinkSprite', pcduino2: 'LinkSprite', pcduino3: 'LinkSprite',
  mba8mp: 'TQ-Systems', mba8mpxl: 'TQ-Systems',
  mkspi: 'Makerbase', mksklipad50: 'Makerbase',
  lubancat2: 'EmbedFire',
  mcimx7sabre: 'NXP',
  microzed: 'Avnet',
  miqi: 'MQmaker',
  rk322x: 'TV boxes', rk3318: 'TV boxes', rk3328: 'TV boxes',
  z28: 'TV boxes', xt: 'TV boxes', mk808c: 'TV boxes', onecloud: 'TV boxes',
};
function canonicalVendor(v) {
  return VENDOR_ALIAS[v.toLowerCase()] ?? v;
}

const files = readdirSync(boardsDir).filter((f) => /\.(conf|csc|wip|eos|tvb)$/.test(f));
const devices = [];
for (const file of files) {
  const ext = file.split('.').pop();
  const id = file.slice(0, -(ext.length + 1));
  const text = readFileSync(join(boardsDir, file), 'utf8');
  const name = val(text, 'BOARD_NAME') || id;
  const family = val(text, 'BOARDFAMILY');
  const desc = firstComment(text) || (family ? `Armbian ${family} board.` : 'Armbian-supported board.');
  const tier = TIER[ext] ?? ext;
  devices.push({
    id,
    name,
    description: desc.length > 140 ? desc.slice(0, 137) + '…' : desc,
    icon: iconFor(family, id, name),
    arch: archFor(family, id, name),
    vendor: canonicalVendor(vendorFor(id, name)),
    tags: [tier, ...(family ? [family] : [])],
  });
}

// curated overrides for the Tier-1 Airwaves boards (nicer copy + badges)
const OVERRIDES = {
  rpi4b: { name: 'Raspberry Pi 3 / 4 / 5', description: 'The most popular feeder board. 64-bit, great SDR support.', icon: 'rpi', vendor: 'Raspberry Pi', tags: ['supported', 'popular'] },
  'rock-5b': { name: 'Radxa Rock 5B', description: 'High-performance RK3588 board for demanding multi-band stations.', icon: 'rock', vendor: 'Radxa', tags: ['supported'] },
  orangepi5: { name: 'Orange Pi 5', description: 'Compact RK3588S board, a strong value feeder.', icon: 'orangepi', vendor: 'Orange Pi', tags: ['supported'] },
  'uefi-x86': { name: 'PC / Mini-PC (x86 UEFI)', description: 'Any 64-bit UEFI PC or mini-PC.', icon: 'x86', arch: 'amd64', vendor: 'PC / x86', tags: ['supported', 'popular'] },
};
for (const d of devices) if (OVERRIDES[d.id]) Object.assign(d, OVERRIDES[d.id]);
devices.sort((a, b) => a.name.localeCompare(b.name));

// ---- Airwaves OS image set ------------------------------------------------
// board, channel, version, downloadSize, extractSize
const IMG = [
  ['rpi4b', 'stable', '1.0.38', 614e6, 2.41e9],
  ['rpi4b', 'stable', '1.0.37', 612e6, 2.40e9],
  ['uefi-x86', 'stable', '1.0.38', 742e6, 2.91e9],
  ['uefi-x86', 'stable', '1.0.37', 740e6, 2.90e9],
  ['rock-5b', 'stable', '1.0.37', 690e6, 2.60e9],
  ['orangepi5', 'stable', '1.0.37', 678e6, 2.55e9],
  ['rpi4b', 'beta', '1.0.39', 616e6, 2.42e9],
  ['uefi-x86', 'beta', '1.0.39', 744e6, 2.92e9],
  // last 3 dev builds
  ['rpi4b', 'dev', '1.0.24', 600e6, 2.38e9],
  ['rpi4b', 'dev', '1.0.23', 599e6, 2.37e9],
  ['rpi4b', 'dev', '1.0.22', 598e6, 2.36e9],
  ['uefi-x86', 'dev', '1.0.24', 730e6, 2.88e9],
  ['uefi-x86', 'dev', '1.0.23', 729e6, 2.87e9],
  ['uefi-x86', 'dev', '1.0.22', 728e6, 2.86e9],
  ['rock-5b', 'dev', '1.0.24', 685e6, 2.59e9],
  ['orangepi5', 'dev', '1.0.24', 675e6, 2.54e9],
];
const RELEASED = {
  '1.0.39': '2026-06-17', '1.0.38': '2026-06-16', '1.0.37': '2026-06-16',
  '1.0.24': '2026-06-10', '1.0.23': '2026-06-10', '1.0.22': '2026-06-10',
};
const ASSET_SLUG = { rpi4b: 'raspberrypi-3-4-5' }; // release asset naming differs from board id
const SHORT = { rpi4b: 'Raspberry Pi', 'rock-5b': 'Rock 5B', orangepi5: 'Orange Pi 5', 'uefi-x86': 'x86 UEFI' };

const images = IMG.map(([device, channel, v, dl, ex]) => {
  const slug = ASSET_SLUG[device] ?? device;
  return {
    id: `${device}-${channel}-${v}`,
    device,
    channel,
    osVersion: v,
    codename: 'Sideband',
    name: `Airwaves OS ${v} (${SHORT[device] ?? device})`,
    released: RELEASED[v],
    notesUrl: `https://github.com/airframesio/airwaves-os/releases/tag/v${v}`,
    url: `https://github.com/airframesio/airwaves-os/releases/download/v${v}/Airwaves_OS_${slug}-${v}.img.xz`,
    compression: 'xz',
    downloadSize: Math.round(dl),
    extractSize: Math.round(ex),
    minStorageBytes: 4 * 1024 * 1024 * 1024,
  };
});
// mark newest per device+channel
const cmp = (a, b) => a.split('.').map(Number).reduce((acc, n, i) => acc || n - b.split('.').map(Number)[i], 0);
for (const img of images) {
  const peers = images.filter((x) => x.device === img.device && x.channel === img.channel);
  img.latest = peers.every((p) => cmp(img.osVersion, p.osVersion) >= 0);
}

// ---- emit -----------------------------------------------------------------
const ts = `// AUTO-GENERATED by scripts/gen-catalog.mjs — do not edit by hand.
// Source: armbian-build/config/boards (${devices.length} boards).
import type { CatalogDevice, CatalogImage } from '../types';

/** Every board Armbian supports, as flasher board options. */
export const armbianDevices: CatalogDevice[] = ${JSON.stringify(devices, null, 2)};

/** Airwaves OS images currently published per board + channel. */
export const airwavesImages: CatalogImage[] = ${JSON.stringify(images, null, 2)};
`;
mkdirSync(join(repo, 'src', 'data'), { recursive: true });
writeFileSync(join(repo, 'src', 'data', 'armbianBoards.ts'), ts);

const catalog = {
  schema: 1,
  name: 'Airwaves OS',
  updated: '2026-06-17T00:00:00Z',
  channels: ['stable', 'beta', 'dev'],
  devices,
  images,
  source: 'remote',
};
writeFileSync(join(repo, 'docs', 'catalog.example.json'), JSON.stringify(catalog, null, 2) + '\n');

console.log(`Generated ${devices.length} devices and ${images.length} images.`);
console.log('  → src/data/armbianBoards.ts');
console.log('  → docs/catalog.example.json');
