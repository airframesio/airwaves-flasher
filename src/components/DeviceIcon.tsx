import { CircuitBoard, Cpu, MonitorCog, Server, HardDrive } from 'lucide-react';

const MAP: Record<string, typeof Cpu> = {
  rpi: CircuitBoard,
  rock: Cpu,
  orangepi: Cpu,
  x86: MonitorCog,
  pc: Server,
  sbc: CircuitBoard,
};

export function DeviceIcon({ icon, size = 24 }: { icon: string; size?: number }) {
  const Ico = MAP[icon] ?? HardDrive;
  return <Ico size={size} strokeWidth={1.6} />;
}
