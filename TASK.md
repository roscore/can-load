# Task: Robot actuator CAN / CAN FD bus load planner (Rust → WebAssembly)

Build an original, small, public static web app named **CAN Load**. Write original code and assets only.
It helps robotics developers answer: "If N joint actuators talk on one CAN / CAN FD bus at a given
control rate, how loaded is the bus, does each control cycle fit, and what bit timing should I set?"

## Product scope
One core feature plus two small extras; target roughly 400–800 lines of Rust and 4–8 unit tests.

### Core feature: bus load + control-cycle fit
- Single-page Korean-first UI with an English toggle.
- Bus settings: protocol (Classic CAN 2.0 / CAN FD), nominal bitrate (125k…1M), FD data bitrate
  (1/2/4/5/8 Mbit/s, FD only), identifier type (11-bit / 29-bit), stuffing model (none / typical /
  worst case), target max load % (e.g. 70 %).
- Message list (editable rows): name, count (e.g. 12 joints), payload bytes (Classic 0–8, FD valid DLC
  sizes 0–8,12,16,20,24,32,48,64 — snap to the next valid size and show it), rate Hz, direction label
  (command / feedback / other), colour chip. Add / delete / duplicate rows.
- Frame model (document it in README): bit count of a data frame for Classic and FD (arbitration phase
  at nominal rate, data phase incl. stuff-count + CRC17/CRC21 at data rate when BRS is used), worst-case
  stuff bits `floor((n-1)/4)` over the stuffable region, plus 3-bit intermission. Compute frame time in µs.
- Results per row and total: bits/frame, µs/frame, frames/s, bus load %, share of the bus; overall
  status pass/warn/fail vs. target; max control rate (Hz) at which all rows sharing the "control" rate
  still fit under target; time to send one full control cycle vs. the cycle period.
- Hero visualization (Canvas 2D, app-drawn, no chart library): a **one-cycle bus timeline**: horizontal
  time axis of one control period, every frame drawn as a coloured block (width ∝ frame time) packed
  back-to-back in priority (row) order, idle gap shown, a dashed line at the target load point,
  hover/focus tooltip with frame details, and a HUD with load %, cycle fill, max rate, status.
  Plus a small load-% vs. control-rate line plot with the target line and current rate marked.
- Explain that it is a planning estimate: arbitration jitter, error frames, retransmissions, bus-off
  recovery, gateway/driver latency and actual transceiver loop delay need measurement.

### Small extras
1. Bit-timing helper: given CAN controller clock (e.g. 40/80 MHz) and desired bitrate + sample point
   (e.g. 80 % / 75 %), list valid (prescaler, tq per bit, TSEG1, TSEG2, SJW) combos with the achieved
   bitrate error and sample point, best first. For FD show nominal and data phase.
2. Three synthetic presets: "12-joint humanoid arm pair @ 1 kHz CAN FD", "4-wheel base @ 200 Hz classic
   CAN", "6-DoF arm @ 500 Hz classic 1 Mbit/s". Label them illustrative and editable. Plus local CSV
   export of the message table (no import, sharing, accounts or storage).

### Polished UI
- Dark design tokens, system fonts, subtle gradient background, full-width timeline hero, slim
  toolbar, compact collapsible settings cards, colour chips per row, 150–200 ms transitions, mobile
  bottom-sheet layout, dismissible first-run hint. Keyboard accessible, visible focus, reduced-motion.
- The initial example must be useful immediately. No 3D needed.

## Architecture and tests
- Workspace with `crates/cl_core` (pure Rust) and `crates/cl_web` (wasm-bindgen/web-sys/js-sys only).
- 4–8 hand-checkable tests: classic 11-bit 8-byte frame = 111 bits without stuffing (incl. 3 IFS) and
  worst-case stuffing count, 29-bit frame length, FD DLC snapping (9→12, 33→48), FD frame time with BRS,
  bus load sum, bit-timing search finds 1 Mbit/s @ 80 % on 40 MHz with 0 % error, CSV escaping.

## Repository / release requirements
- `README.md`: Korean first, then English; what it does, model/limitations, controls, local build,
  privacy, MIT.
- MIT `LICENSE`, copyright `can-load contributors`. `.gitignore` for target and dist.
- `Trunk.toml` with `public_url = "/can-load/"`, `filehash = false`.
- `index.html` CSP: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; object-src 'none'; base-uri 'self'`.
- `.github/workflows/pages.yml` (already present, official Pages actions) and `tools/privacy_check.sh`
  (already present) must keep working; the privacy check must pass.
- Commit identity: `can-load contributors <59640857+roscore@users.noreply.github.com>`.
- Do not push to main. Commit the implementation on a branch and open a PR.

## Done when
- `cargo test` passes with 4–8 tests; `trunk build --release` succeeds; `bash tools/privacy_check.sh` clean.
- App works at the `/can-load/` subpath with no external runtime requests.
- README (KO + EN), LICENSE, Pages workflow present. A PR is open; do not change repo settings.
