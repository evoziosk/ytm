export interface Line {
  time: number;
  text: string;
}
export function parseLrc(source: string): Line[] {
  const lines: Line[] = [];
  for (const line of source.split('\n')) {
    const stamps = [...line.matchAll(/\[(\d+):(\d{2})(?:\.(\d{1,3}))?\]/g)];
    const text = line.replace(/\[[^\]]*\]/g, '').trim();
    for (const stamp of stamps)
      lines.push({
        time:
          Number(stamp[1]) * 60 +
          Number(stamp[2]) +
          Number(`0.${stamp[3] ?? '0'}`),
        text,
      });
  }
  return lines.sort((a, b) => a.time - b.time);
}
