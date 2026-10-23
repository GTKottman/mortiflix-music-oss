// A Type-1 Standard MIDI File from the studio's notes ({ t, len, key, vel } in ticks at 960 per beat, vel 0-1): a
// conductor track (tempo, meter, section markers), then one track per part with its name and General MIDI program;
// drums on channel 10.
export const PPQ = 960;

function vlq(n) {
  const bytes = [n & 0x7f];
  while ((n >>= 7)) bytes.unshift((n & 0x7f) | 0x80);
  return bytes;
}
const chunk = (type, data) => {
  const head = Buffer.alloc(8);
  head.write(type, 0, 'latin1');
  head.writeUInt32BE(data.length, 4);
  return Buffer.concat([head, Buffer.from(data)]);
};
const meta = (type, s) => { const b = [...Buffer.from(String(s).slice(0, 120), 'utf8')]; return [0xff, type, ...vlq(b.length), ...b]; };

// parts: [{ name, program, drums, notes }]; markers: [{ t, text }] in ticks.
export function writeSmf({ bpm, meter = '4/4', parts, markers = [] }) {
  const tempo = Math.round(60_000_000 / bpm);
  const [num, den] = String(meter).split('/').map(Number);
  const conductor = [0, ...meta(0x03, 'tempo'), 0, 0xff, 0x51, 3, (tempo >> 16) & 0xff, (tempo >> 8) & 0xff, tempo & 0xff,
    0, 0xff, 0x58, 4, num || 4, Math.log2(den || 4), 24, 8];
  let at = 0;
  for (const m of [...markers].sort((a, b) => a.t - b.t)) {
    const tick = Math.max(0, Math.round(m.t));
    conductor.push(...vlq(tick - at), ...meta(0x06, m.text));
    at = tick;
  }
  conductor.push(0, 0xff, 0x2f, 0);
  const out = [chunk('MTrk', conductor)];
  let next = 0;
  for (const p of parts) {
    const ch = p.drums ? 9 : [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15][next++ % 15];
    const events = [];
    for (const n of p.notes) {
      const on = Math.max(0, Math.round(n.t));
      const off = Math.max(on + 1, Math.round(n.t + n.len));
      const vel = Math.max(1, Math.min(127, Math.round(n.vel * 127)));
      events.push({ tick: on, data: [0x90 | ch, n.key, vel], order: 1 }, { tick: off, data: [0x80 | ch, n.key, 0], order: 0 });
    }
    events.sort((a, b) => a.tick - b.tick || a.order - b.order);
    const bytes = [0, ...meta(0x03, p.name)];
    if (!p.drums) bytes.push(0, 0xc0 | ch, Math.max(0, Math.min(127, p.program | 0)));
    let last = 0;
    for (const e of events) { bytes.push(...vlq(e.tick - last), ...e.data); last = e.tick; }
    bytes.push(0, 0xff, 0x2f, 0);
    out.push(chunk('MTrk', bytes));
  }
  const header = Buffer.alloc(6);
  header.writeUInt16BE(1, 0); header.writeUInt16BE(out.length, 2); header.writeUInt16BE(PPQ, 4);
  return Buffer.concat([chunk('MThd', header), ...out]);
}
