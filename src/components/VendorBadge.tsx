/**
 * A vendor "logo" badge. If a logo file exists at
 * `src/assets/vendors/<slug>.{png,svg,webp}` it is shown on a white tile;
 * otherwise we fall back to a clean monogram in a per-vendor tinted square.
 * Logos are the manufacturers' own brand marks (fetched from their official
 * sites); slugs without a known real brand (SoC names, generic categories)
 * keep the monogram. Drop-in: add a `<slug>.png` and it's picked up
 * automatically (no code change, no failed network requests when absent).
 */

// Eagerly resolve any vendor logo assets that happen to be present.
const logoUrls = import.meta.glob('../assets/vendors/*.{png,svg,webp}', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>;

const logoBySlug: Record<string, string> = {};
for (const [path, url] of Object.entries(logoUrls)) {
  const slug = path.split('/').pop()!.replace(/\.(png|svg|webp)$/, '');
  logoBySlug[slug] = url;
}

export function vendorSlug(vendor: string): string {
  return vendor.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '');
}

function initials(name: string): string {
  const words = name.trim().split(/[\s/]+/).filter(Boolean);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  return name.replace(/[^A-Za-z0-9]/g, '').slice(0, 2).toUpperCase() || '·';
}

function hue(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 360;
}

export function VendorBadge({ vendor, size = 40 }: { vendor: string; size?: number }) {
  const slug = vendorSlug(vendor);
  const logo = logoBySlug[slug];
  return (
    <span
      className="vbadge"
      style={{ width: size, height: size, ['--vh' as string]: hue(vendor) }}
      aria-hidden
    >
      {logo ? (
        <img src={logo} alt="" className="vbadge__img" />
      ) : (
        <span className="vbadge__mono">{initials(vendor)}</span>
      )}
    </span>
  );
}
