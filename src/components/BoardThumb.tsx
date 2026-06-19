/**
 * Board thumbnail: shows the board's product photo if one is bundled at
 * `src/assets/boards/<id>.png`, otherwise falls back to the generic DeviceIcon.
 * Photos are the Armbian board images (fetched per board id); boards without a
 * published photo keep the icon. Drop-in: add a `<id>.png` and it appears.
 */
import { DeviceIcon } from './DeviceIcon';

const boardUrls = import.meta.glob('../assets/boards/*.png', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>;

const photoById: Record<string, string> = {};
for (const [path, url] of Object.entries(boardUrls)) {
  const id = path.split('/').pop()!.replace(/\.png$/, '');
  photoById[id] = url;
}

export function hasBoardPhoto(id: string): boolean {
  return !!photoById[id];
}

export function BoardThumb({
  id,
  icon,
  size = 'card',
}: {
  id: string;
  icon: string;
  size?: 'card' | 'sm';
}) {
  const photo = photoById[id];
  return (
    <span
      className={`boardthumb boardthumb--${size} ${photo ? 'boardthumb--photo' : ''}`}
      aria-hidden
    >
      {photo ? <img src={photo} alt="" className="boardthumb__img" /> : <DeviceIcon icon={icon} />}
    </span>
  );
}
