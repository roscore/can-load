//! CAN / CAN FD planning model: frame bit counts, frame time, bus load,
//! control-cycle fit, bit-timing search, presets and CSV export.
//! All numbers are planning estimates (see README "Frame model").

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Proto { Classic, Fd }
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stuff { None, Typical, Worst }
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dir { Command, Feedback, Other }

impl Dir {
    pub fn key(self) -> &'static str {
        match self { Dir::Command => "command", Dir::Feedback => "feedback", Dir::Other => "other" }
    }
    pub fn from_key(s: &str) -> Dir {
        match s { "command" => Dir::Command, "feedback" => Dir::Feedback, _ => Dir::Other }
    }
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub proto: Proto,
    pub nominal: f64,
    pub data: f64,
    pub ext_id: bool,
    pub stuff: Stuff,
    pub target_pct: f64,
}

#[derive(Clone, Debug)]
pub struct Msg {
    pub name: String,
    pub count: u32,
    pub bytes: u32,
    pub rate_hz: f64,
    pub dir: Dir,
    pub color: String,
}

pub const FD_SIZES: [u32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];

/// Snap a payload size to what the protocol can carry (next valid DLC size).
pub fn snap_payload(proto: Proto, bytes: u32) -> u32 {
    match proto {
        Proto::Classic => bytes.min(8),
        Proto::Fd => *FD_SIZES.iter().find(|&&s| s >= bytes).unwrap_or(&64),
    }
}

/// Bits of one data frame, split by phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBits {
    /// Bits sent at the nominal (arbitration) rate, incl. stuff bits and 3-bit IFS.
    pub nominal: u32,
    /// Bits sent at the data rate (FD with BRS only; 0 otherwise).
    pub data: u32,
    /// Stuff bits included above (dynamic + FD fixed stuff bits).
    pub stuff: u32,
}

impl FrameBits {
    pub fn total(&self) -> u32 { self.nominal + self.data }
}

fn dyn_stuff(model: Stuff, n: u32) -> u32 {
    let worst = n.saturating_sub(1) / 4;
    match model { Stuff::None => 0, Stuff::Typical => worst / 2, Stuff::Worst => worst }
}

pub fn frame_bits(bus: &Bus, payload: u32) -> FrameBits {
    let n = snap_payload(bus.proto, payload);
    // ACK slot + ACK delimiter + EOF(7) + intermission(3)
    let tail = 1 + 1 + 7 + 3;
    match bus.proto {
        Proto::Classic => {
            // SOF..DLC: base 19 (SOF,ID11,RTR,IDE,r0,DLC4); ext 39 (SOF,ID11,SRR,IDE,ID18,RTR,r1,r0,DLC4)
            let head = if bus.ext_id { 39 } else { 19 };
            let stuffable = head + 8 * n + 15; // ..CRC15
            let s = dyn_stuff(bus.stuff, stuffable);
            FrameBits { nominal: stuffable + s + 1 + tail, data: 0, stuff: s }
        }
        Proto::Fd => {
            // Arbitration up to and incl. BRS: base SOF,ID11,RRS,IDE,FDF,res,BRS = 17; ext 36
            let arb = if bus.ext_id { 36 } else { 17 };
            let ctl = 1 + 4 + 8 * n; // ESI + DLC + data (dynamically stuffed, data rate)
            let crc = if n <= 16 { 17 } else { 21 };
            let fixed = (4 + crc) / 4 + 1; // fixed stuff bits in stuff-count + CRC field
            let total_s = dyn_stuff(bus.stuff, arb + ctl);
            let arb_s = dyn_stuff(bus.stuff, arb).min(total_s);
            let data_bits = ctl + (total_s - arb_s) + 4 + crc + fixed + 1; // + CRC delimiter
            let arb_bits = arb + arb_s + tail;
            let stuff = total_s + fixed;
            if bus.data > bus.nominal {
                FrameBits { nominal: arb_bits, data: data_bits, stuff }
            } else {
                FrameBits { nominal: arb_bits + data_bits, data: 0, stuff }
            }
        }
    }
}

/// Frame time in microseconds.
pub fn frame_us(bus: &Bus, payload: u32) -> f64 {
    let b = frame_bits(bus, payload);
    b.nominal as f64 * 1e6 / bus.nominal + b.data as f64 * 1e6 / bus.data
}

#[derive(Clone, Debug)]
pub struct RowResult {
    pub bytes: u32,
    pub bits: u32,
    pub us: f64,
    pub fps: f64,
    pub load_pct: f64,
    pub share_pct: f64,
    pub control: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status { Pass, Warn, Fail }

#[derive(Clone, Debug)]
pub struct Summary {
    pub rows: Vec<RowResult>,
    pub load_pct: f64,
    pub fps: f64,
    pub control_hz: f64,
    pub period_us: f64,
    /// Time of all control-rate frames in one cycle.
    pub cycle_us: f64,
    /// Max control rate keeping total load <= target (other rows fixed).
    pub max_hz: f64,
    /// Load with the control rows at a given rate: other_pct + per_hz_pct * f.
    pub other_pct: f64,
    pub per_hz_pct: f64,
    pub status: Status,
}

/// Total bus load in percent for a list of messages.
pub fn bus_load_pct(bus: &Bus, msgs: &[Msg]) -> f64 {
    msgs.iter().map(|m| m.count as f64 * m.rate_hz * frame_us(bus, m.bytes)).sum::<f64>() / 1e4
}

/// The "control rate" is the fastest row rate; every row at that rate is a control row.
pub fn analyze(bus: &Bus, msgs: &[Msg]) -> Summary {
    let control_hz = msgs.iter().map(|m| m.rate_hz).fold(0.0, f64::max);
    let mut rows = Vec::new();
    let (mut load, mut fps, mut cycle_us, mut other) = (0.0, 0.0, 0.0, 0.0);
    for m in msgs {
        let fb = frame_bits(bus, m.bytes);
        let us = frame_us(bus, m.bytes);
        let f = m.count as f64 * m.rate_hz.max(0.0);
        let l = f * us / 1e4;
        let control = control_hz > 0.0 && (m.rate_hz - control_hz).abs() < 1e-9;
        if control { cycle_us += m.count as f64 * us } else { other += l }
        load += l;
        fps += f;
        rows.push(RowResult { bytes: snap_payload(bus.proto, m.bytes), bits: fb.total(), us, fps: f,
            load_pct: l, share_pct: 0.0, control });
    }
    for r in &mut rows { r.share_pct = if load > 0.0 { r.load_pct / load * 100.0 } else { 0.0 } }
    let per_hz = cycle_us / 1e4;
    let max_hz = if per_hz > 0.0 { ((bus.target_pct - other) / per_hz).max(0.0) } else { 0.0 };
    let period_us = if control_hz > 0.0 { 1e6 / control_hz } else { 0.0 };
    let status = if load > 100.0 || cycle_us > period_us { Status::Fail }
        else if load > bus.target_pct { Status::Warn } else { Status::Pass };
    Summary { rows, load_pct: load, fps, control_hz, period_us, cycle_us, max_hz, other_pct: other,
        per_hz_pct: per_hz, status }
}

// ---------------------------------------------------------------- bit timing

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub prescaler: u32,
    pub tq: u32,
    pub tseg1: u32,
    pub tseg2: u32,
    pub sjw: u32,
    pub bitrate: f64,
    pub err_pct: f64,
    pub sp_pct: f64,
}

/// Limits typical of CAN FD controllers: (max prescaler, max TSEG1, max TSEG2, max SJW).
pub const NOMINAL_LIMITS: (u32, u32, u32, u32) = (512, 256, 128, 128);
pub const DATA_LIMITS: (u32, u32, u32, u32) = (32, 32, 16, 16);

/// Valid timings within 0.5 % bitrate error, best first (error, then sample point, then more tq).
pub fn bit_timings(clock_hz: f64, bitrate: f64, sp_pct: f64, lim: (u32, u32, u32, u32)) -> Vec<Timing> {
    let mut out = Vec::new();
    if clock_hz <= 0.0 || bitrate <= 0.0 { return out; }
    for p in 1..=lim.0 {
        let tq = (clock_hz / (p as f64 * bitrate)).round() as u32;
        if tq < 4 || tq > 1 + lim.1 + lim.2 { continue; }
        let tseg1 = ((tq as f64 * sp_pct / 100.0).round() as u32).saturating_sub(1).clamp(1, lim.1);
        let Some(tseg2) = (tq - 1).checked_sub(tseg1) else { continue };
        if tseg2 < 1 || tseg2 > lim.2 { continue; }
        let br = clock_hz / (p as f64 * tq as f64);
        let err = (br - bitrate) / bitrate * 100.0;
        if err.abs() > 0.5 { continue; }
        out.push(Timing { prescaler: p, tq, tseg1, tseg2, sjw: tseg2.min(lim.3), bitrate: br, err_pct: err,
            sp_pct: (1 + tseg1) as f64 / tq as f64 * 100.0 });
    }
    out.sort_by(|a, b| {
        let ka = (a.err_pct.abs(), (a.sp_pct - sp_pct).abs());
        let kb = (b.err_pct.abs(), (b.sp_pct - sp_pct).abs());
        ka.partial_cmp(&kb).unwrap().then(b.tq.cmp(&a.tq))
    });
    out
}

// ---------------------------------------------------------------- presets + CSV

fn m(name: &str, count: u32, bytes: u32, rate: f64, dir: Dir, color: &str) -> Msg {
    Msg { name: name.into(), count, bytes, rate_hz: rate, dir, color: color.into() }
}

/// Illustrative, synthetic presets: (key, bus, messages).
pub fn presets() -> Vec<(&'static str, Bus, Vec<Msg>)> {
    let fd = Bus { proto: Proto::Fd, nominal: 1e6, data: 5e6, ext_id: false, stuff: Stuff::Worst, target_pct: 70.0 };
    let cl = |rate| Bus { proto: Proto::Classic, nominal: rate, data: rate, ext_id: false, stuff: Stuff::Worst, target_pct: 70.0 };
    vec![
        ("humanoid", fd, vec![
            m("Arm command (6 joints packed)", 2, 48, 1000.0, Dir::Command, "#5b9cff"),
            m("Arm feedback (6 joints packed)", 2, 64, 1000.0, Dir::Feedback, "#3ad1a4"),
            m("Wrist force/torque", 2, 16, 500.0, Dir::Feedback, "#f2b84b"),
            m("Joint diagnostics", 12, 8, 10.0, Dir::Other, "#c58cff"),
        ]),
        ("base", cl(500e3), vec![
            m("Wheel command", 4, 8, 200.0, Dir::Command, "#5b9cff"),
            m("Wheel feedback", 4, 8, 200.0, Dir::Feedback, "#3ad1a4"),
            m("Battery / BMS", 1, 8, 10.0, Dir::Other, "#f2b84b"),
        ]),
        ("arm6", cl(1e6), vec![
            m("Joint command", 6, 8, 500.0, Dir::Command, "#5b9cff"),
            m("Joint feedback", 6, 8, 500.0, Dir::Feedback, "#3ad1a4"),
            m("Gripper", 1, 4, 100.0, Dir::Other, "#f2b84b"),
            m("Heartbeat", 6, 1, 10.0, Dir::Other, "#c58cff"),
        ]),
    ]
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

pub fn to_csv(bus: &Bus, msgs: &[Msg]) -> String {
    let s = analyze(bus, msgs);
    let mut out = String::from("name,count,payload_bytes,rate_hz,direction,bits_per_frame,us_per_frame,frames_per_s,load_pct,share_pct\n");
    for (msg, r) in msgs.iter().zip(&s.rows) {
        out += &format!("{},{},{},{},{},{},{:.2},{:.0},{:.2},{:.1}\n", csv_field(&msg.name), msg.count, r.bytes,
            msg.rate_hz, msg.dir.key(), r.bits, r.us, r.fps, r.load_pct, r.share_pct);
    }
    out += &format!("TOTAL,,,,,,,{:.0},{:.2},100.0\n", s.fps, s.load_pct);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bus(proto: Proto, ext: bool, stuff: Stuff, data: f64) -> Bus {
        Bus { proto, nominal: 1e6, data, ext_id: ext, stuff, target_pct: 70.0 }
    }

    #[test]
    fn classic_11bit_8byte() {
        assert_eq!(frame_bits(&bus(Proto::Classic, false, Stuff::None, 1e6), 8).total(), 111);
        // stuffable SOF..CRC = 98 bits -> floor(97/4) = 24 worst-case stuff bits
        let w = frame_bits(&bus(Proto::Classic, false, Stuff::Worst, 1e6), 8);
        assert_eq!((w.stuff, w.total()), (24, 135));
    }

    #[test]
    fn classic_29bit_8byte() {
        assert_eq!(frame_bits(&bus(Proto::Classic, true, Stuff::None, 1e6), 8).total(), 131);
        assert_eq!(frame_bits(&bus(Proto::Classic, true, Stuff::Worst, 1e6), 8).total(), 160);
    }

    #[test]
    fn fd_dlc_snapping() {
        assert_eq!(snap_payload(Proto::Fd, 9), 12);
        assert_eq!(snap_payload(Proto::Fd, 33), 48);
        assert_eq!(snap_payload(Proto::Fd, 99), 64);
        assert_eq!(snap_payload(Proto::Classic, 12), 8);
    }

    #[test]
    fn fd_64byte_brs_time() {
        // nominal: 17 arb + 12 tail = 29 bits @ 1 Mbit/s = 29 us
        // data: ESI+DLC 5 + 512 + stuff count 4 + CRC21 + 7 fixed + delim 1 = 550 bits @ 5 Mbit/s = 110 us
        let b = bus(Proto::Fd, false, Stuff::None, 5e6);
        assert_eq!(frame_bits(&b, 64), FrameBits { nominal: 29, data: 550, stuff: 7 });
        assert!((frame_us(&b, 64) - 139.0).abs() < 1e-9);
    }

    #[test]
    fn bus_load_sum() {
        // 10 x 111 us @ 500 Hz = 55.5 %, + 1 x 111 us @ 100 Hz = 1.11 %
        let b = bus(Proto::Classic, false, Stuff::None, 1e6);
        let msgs = [m("a", 10, 8, 500.0, Dir::Command, "#fff"), m("b", 1, 8, 100.0, Dir::Other, "#fff")];
        assert!((bus_load_pct(&b, &msgs) - 56.61).abs() < 1e-9);
        let s = analyze(&b, &msgs);
        assert!((s.cycle_us - 1110.0).abs() < 1e-9 && s.status == Status::Pass);
        assert!((s.max_hz - (70.0 - 1.11) / 0.111).abs() < 1e-6);
    }

    #[test]
    fn bit_timing_1m_80pct_40mhz() {
        let best = bit_timings(40e6, 1e6, 80.0, NOMINAL_LIMITS)[0];
        assert_eq!((best.prescaler, best.tq, best.tseg1, best.tseg2), (1, 40, 31, 8));
        assert!(best.err_pct.abs() < 1e-12 && (best.sp_pct - 80.0).abs() < 1e-12);
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
