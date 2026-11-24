# CAN Load

로봇 관절 액추에이터용 **CAN / CAN FD 버스 부하 계획 도구**입니다. Rust → WebAssembly로 만든 작은 정적 웹 앱이며, 모든 계산은 브라우저 안에서만 이루어집니다.

> 한국어 설명 다음에 English section이 이어집니다.

## 무엇을 하나요

"N개의 관절 액추에이터가 하나의 CAN / CAN FD 버스에서 주어진 제어 주기로 통신하면, 버스는 얼마나 차고, 제어 1주기 안에 다 들어가며, 비트 타이밍은 어떻게 설정해야 하나?"에 답합니다.

- **버스 부하 + 제어 주기 적합성 (핵심 기능)**: 프로토콜(Classic CAN 2.0 / CAN FD), 공칭 비트레이트(125k…1M), FD 데이터 비트레이트(1/2/4/5/8 Mbit/s), 11/29-bit ID, 스터핑 모델(없음/일반/최악), 목표 최대 부하 %를 설정하고, 메시지 표(이름, 개수, 페이로드, 주기 Hz, 방향, 색상)를 편집합니다. 행별·전체 비트/프레임, µs/프레임, 프레임/s, 부하 %, 점유율, 통과/주의/실패 판정, 목표 내 최대 제어 주기, 1주기 전송 시간 대비 주기 길이를 보여 줍니다.
- **제어 1주기 버스 타임라인 (Canvas 2D)**: 한 제어 주기를 가로축으로, 모든 프레임을 시간에 비례하는 색 블록으로 우선순위(행 순서)대로 이어 그립니다. 유휴 구간, 목표 부하 지점의 점선, 호버/키보드 포커스 툴팁, HUD(부하 %, 주기 점유, 최대 주기, 판정)를 제공합니다. 아래에는 제어 주기에 따른 부하 % 선 그래프가 있습니다.
- **비트 타이밍 도우미**: 컨트롤러 클럭(예: 40/80 MHz), 목표 비트레이트, 샘플 포인트(예: 80 % / 75 %)로 유효한 (prescaler, 비트당 tq, TSEG1, TSEG2, SJW) 조합을 오차가 작은 순서로 나열합니다. CAN FD는 공칭/데이터 위상을 따로 보여 줍니다.
- **예시 3종 + CSV 내보내기**: "12관절 휴머노이드 양팔 @ 1 kHz CAN FD", "4륜 베이스 @ 200 Hz 클래식 CAN", "6자유도 팔 @ 500 Hz 클래식 1 Mbit/s". 모두 **설명용 가상 예시**이며 자유롭게 수정할 수 있습니다. 메시지 표는 로컬 CSV 파일로 내보낼 수 있습니다(가져오기·공유·계정·저장 기능 없음).

## 프레임 모델

데이터 프레임(리모트 프레임 아님) 하나의 비트 수를 다음과 같이 셉니다. `n` = 페이로드 바이트(Classic은 0–8로 제한, FD는 0–8, 12, 16, 20, 24, 32, 48, 64 중 **다음 유효 크기로 올림**하고 표에 `→12`처럼 표시).

**Classic CAN**

| 구간 | 11-bit | 29-bit |
|---|---|---|
| SOF ~ DLC | 19 (SOF, ID11, RTR, IDE, r0, DLC4) | 39 (SOF, ID11, SRR, IDE, ID18, RTR, r1, r0, DLC4) |
| 데이터 | 8n | 8n |
| CRC | 15 | 15 |
| CRC 구분자, ACK, ACK 구분자, EOF(7) | 10 | 10 |
| 인터미션(IFS) | 3 | 3 |

- 스터핑 없음: 11-bit 8바이트 = **111 비트**, 29-bit 8바이트 = **131 비트**.
- 스터핑 대상 구간: SOF ~ CRC 끝 (`19 + 8n + 15` 또는 `39 + 8n + 15` 비트). 최악의 스터프 비트 수 = `floor((N − 1) / 4)`. 11-bit 8바이트는 24비트가 추가되어 135비트, 29-bit 8바이트는 29비트가 추가되어 160비트입니다.
- "일반" 모델은 최악값의 절반(내림)을 쓰는 단순 근사입니다.

**CAN FD** (BRS 사용 시 데이터 비트레이트 > 공칭 비트레이트)

- 공칭 속도 구간: SOF ~ BRS (11-bit: SOF, ID11, RRS, IDE, FDF, res, BRS = 17, 29-bit: 36) + 그 구간의 스터프 비트 + ACK, ACK 구분자, EOF(7), IFS(3) = 12비트.
- 데이터 속도 구간: ESI + DLC(4) + 8n + 데이터 구간 스터프 비트 + 스터프 카운트(4) + CRC17(n ≤ 16) 또는 CRC21(n > 16) + 고정 스터프 비트(CRC17은 6개, CRC21은 7개) + CRC 구분자.
- 동적 스터핑은 SOF ~ 데이터 끝 구간에만 적용합니다: 전체 `floor((N − 1) / 4)`를 구하고, 공칭 구간 몫은 `floor((N_arb − 1) / 4)`, 나머지를 데이터 구간에 배정합니다. 고정 스터프 비트는 스터핑 모델과 관계없이 항상 포함합니다.
- 근사: BRS 비트와 CRC 구분자의 샘플 포인트에서 속도가 바뀌는 부분은 각각 공칭/데이터 구간 한쪽에 통째로 넣었습니다. 데이터 비트레이트가 공칭과 같으면 BRS 없이 전체를 공칭 속도로 계산합니다.
- 예: 11-bit, 64바이트, 1 / 5 Mbit/s, 스터핑 없음 → 29비트 × 1 µs + 550비트 × 0.2 µs = **139 µs**.

**부하와 제어 주기**

- 프레임 시간 = 공칭 비트 / 공칭 비트레이트 + 데이터 비트 / 데이터 비트레이트.
- 행 부하 % = 개수 × 주기 Hz × 프레임 시간 / 1 s × 100, 전체 부하는 합.
- **제어 주기 행** = 가장 빠른 주기(Hz)를 가진 모든 행. 1주기 전송 시간은 이 행들의 프레임 시간 합이고, 주기 길이는 `1 / 제어 주기`입니다.
- 목표 내 최대 제어 주기 = (목표 % − 다른 행 부하 %) / (제어 행의 Hz당 부하 %). 다른 행은 주기를 고정한 채로 둡니다.
- 판정: 부하 ≤ 목표이면 통과, 목표 초과 ~ 100 %이면 주의, 100 % 초과이거나 제어 프레임이 주기 안에 다 들어가지 않으면 실패.
- 타임라인에서 더 느린 행은 주기당 평균 점유 시간만큼 흐린 블록으로 그립니다.

## 한계

이 값은 **계획용 추정치**입니다. 중재 지터, 에러 프레임, 재전송, bus-off 복구, 게이트웨이/드라이버 지연, 실제 트랜시버 루프 지연, 컨트롤러 TX 큐 동작은 모델에 포함되지 않으므로 실제 하드웨어에서 측정해야 합니다. 비트 타이밍 범위(prescaler ≤ 512/32, TSEG1 ≤ 256/32, TSEG2 ≤ 128/16)는 일반적인 CAN FD 컨트롤러 기준이며, 사용하는 컨트롤러의 데이터시트를 확인하세요.

## 조작법

- 상단 바: 예시 선택, CSV 내보내기, 언어 전환(한국어/English), 모바일에서는 설정 시트 열기.
- 메시지 표: 값을 바꾸면 즉시 반영됩니다. 행 끝의 `⧉`는 복제, `✕`는 삭제, 아래의 "+ 행 추가"로 새 행을 만듭니다. 색상 칩으로 타임라인 블록 색을 바꿉니다.
- 타임라인: 마우스를 올리면 프레임 정보가 나옵니다. Tab으로 포커스한 뒤 ←/→, Home/End로 프레임을 이동하고 Esc로 닫습니다.
- 설정 카드는 접고 펼 수 있으며, 좁은 화면에서는 하단 시트로 바뀝니다.

## 로컬 빌드

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.22.2
cargo test
trunk build --release      # 결과물: dist/ (public_url = /can-load/)
trunk serve                # 개발 서버
bash tools/privacy_check.sh
```

## 개인정보

서버 구성 요소, 분석 도구, 추적기, 외부 글꼴/CDN, 텔레메트리, 런타임 네트워크 요청이 없습니다. 입력한 값은 브라우저 메모리에만 있으며 저장되거나 전송되지 않습니다. CSV는 브라우저에서 직접 만들어 내려받습니다. 콘텐츠 보안 정책(CSP)이 외부 출처를 차단합니다.

## 라이선스

MIT — `LICENSE` 참고. Copyright (c) can-load contributors.

---

## English

**CAN Load** is a small static web app (Rust → WebAssembly) for planning **CAN / CAN FD bus load for robot joint actuators**. Everything runs in your browser.

### What it does

It answers: "If N joint actuators talk on one CAN / CAN FD bus at a given control rate, how loaded is the bus, does each control cycle fit, and what bit timing should I set?"

- **Bus load + control-cycle fit (core)**: protocol (Classic CAN 2.0 / CAN FD), nominal bitrate (125k…1M), FD data bitrate (1/2/4/5/8 Mbit/s), 11/29-bit identifiers, stuffing model (none / typical / worst case), target max load %. Editable message rows (name, count, payload, rate Hz, direction, colour). Per-row and total bits/frame, µs/frame, frames/s, load %, share, pass/warn/fail status, max control rate under target, and one-cycle send time vs. the cycle period.
- **One-cycle bus timeline (Canvas 2D)**: one control period on the time axis, every frame as a coloured block (width ∝ frame time) packed back-to-back in row (priority) order, the idle gap, a dashed line at the target load point, a hover/keyboard tooltip and a HUD (load %, cycle fill, max rate, status). Below it, a load % vs. control rate plot with the target line and the current rate marked.
- **Bit-timing helper**: from the controller clock (e.g. 40/80 MHz), bitrate and sample point (e.g. 80 % / 75 %), lists valid (prescaler, tq per bit, TSEG1, TSEG2, SJW) combinations with achieved bitrate error and sample point, best first. CAN FD shows the nominal and the data phase.
- **Three presets + CSV export**: "12-joint humanoid arm pair @ 1 kHz CAN FD", "4-wheel base @ 200 Hz classic CAN", "6-DoF arm @ 500 Hz classic 1 Mbit/s". They are **synthetic, illustrative** and fully editable. The message table can be exported as a local CSV file (no import, sharing, accounts or storage).

### Frame model

Bits of one data frame, with `n` payload bytes (Classic clamps to 0–8; FD **snaps up to the next valid size** 0–8, 12, 16, 20, 24, 32, 48, 64 and shows e.g. `→12` in the table).

**Classic CAN**: SOF..DLC is 19 bits (11-bit ID) or 39 bits (29-bit ID), then 8n data bits, CRC 15, CRC delimiter + ACK + ACK delimiter + EOF(7) = 10, and 3 bits of intermission. Without stuffing an 11-bit 8-byte frame is **111 bits** and a 29-bit 8-byte frame is **131 bits**. The stuffable region is SOF to end of CRC; worst-case stuff bits are `floor((N − 1) / 4)`: +24 bits (135) for 11-bit/8 bytes, +29 bits (160) for 29-bit/8 bytes. "Typical" uses half of the worst case (rounded down), a simple approximation.

**CAN FD** with BRS (data bitrate > nominal):
- Nominal-rate part: SOF..BRS (17 bits for 11-bit IDs, 36 for 29-bit) plus its stuff bits, plus ACK, ACK delimiter, EOF(7) and IFS(3) = 12 bits.
- Data-rate part: ESI + DLC(4) + 8n + data-phase stuff bits + stuff count (4) + CRC17 (n ≤ 16) or CRC21 (n > 16) + fixed stuff bits (6 for CRC17, 7 for CRC21) + CRC delimiter.
- Dynamic stuffing covers SOF to end of data only: the total is `floor((N − 1) / 4)`, the nominal share is `floor((N_arb − 1) / 4)` and the rest goes to the data phase. Fixed stuff bits are always counted, whatever the stuffing model.
- Approximation: the bit-rate switch inside the BRS bit and the CRC delimiter is not split; each bit is counted wholly in one phase. If the data bitrate equals the nominal bitrate, the whole frame is timed at the nominal rate (no BRS).
- Example: 11-bit ID, 64 bytes, 1 / 5 Mbit/s, no stuffing → 29 bits × 1 µs + 550 bits × 0.2 µs = **139 µs**.

**Load and control cycle**
- Frame time = nominal bits / nominal bitrate + data bits / data bitrate.
- Row load % = count × rate × frame time / 1 s × 100; the total is the sum.
- **Control rows** are all rows at the fastest rate. One-cycle send time is the sum of their frame times; the period is `1 / control rate`.
- Max control rate under target = (target % − load % of other rows) / (load % per Hz of the control rows); other rows keep their rates.
- Status: pass if load ≤ target; warn if above target up to 100 %; fail above 100 % or when the control frames do not fit in one period.
- In the timeline, slower rows are drawn faded, sized to their average time per cycle.

### Limitations

This is a **planning estimate**. Arbitration jitter, error frames, retransmissions, bus-off recovery, gateway/driver latency, the actual transceiver loop delay and controller TX queue behaviour are not modelled and need measurement on real hardware. Bit-timing limits (prescaler ≤ 512/32, TSEG1 ≤ 256/32, TSEG2 ≤ 128/16) follow typical CAN FD controllers; check your controller's datasheet.

### Controls

- Top bar: preset, CSV export, language toggle (한국어 / English) and, on phones, the settings sheet.
- Message table: edits apply instantly; `⧉` duplicates a row, `✕` deletes it, "+ Add row" adds one; the colour chip sets the block colour.
- Timeline: hover for frame details, or Tab to focus it and use ←/→, Home/End to step through frames, Esc to clear.
- Settings cards collapse; on narrow screens they become a bottom sheet.

### Local build

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.22.2
cargo test
trunk build --release      # output: dist/ (public_url = /can-load/)
trunk serve                # dev server
bash tools/privacy_check.sh
```

### Privacy

No server component, analytics, trackers, external fonts/CDNs, telemetry or runtime network requests. Values you enter stay in browser memory and are never stored or sent. The CSV is generated and downloaded locally. A Content Security Policy blocks other origins.

### License

MIT — see `LICENSE`. Copyright (c) can-load contributors.
