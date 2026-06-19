import markUrl from '../assets/brand/airwaves.png';

export function Brand({ sub = true }: { sub?: boolean }) {
  return (
    <span className="brand">
      <img className="brand__mark" src={markUrl} alt="" />
      <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.05 }}>
        <span className="brand__name">
          Airwaves<b> OS</b>
        </span>
        {sub && <span className="brand__sub">Flasher</span>}
      </span>
    </span>
  );
}
