//! Browser front-end: DOM wiring, Canvas 2D timeline and rate plot. All state stays in memory.
use cl_core::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d as Ctx, Document, Element, Event, HtmlCanvasElement, HtmlElement,
    HtmlInputElement, HtmlSelectElement, KeyboardEvent, MouseEvent};

struct Block { x0: f64, x1: f64, row: usize, k: u32, avg: bool }

struct App { bus: Bus, msgs: Vec<Msg>, en: bool, blocks: Vec<Block>, sel: Option<usize> }

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

fn with<R>(f: impl FnOnce(&mut App) -> R) -> R { APP.with(|a| f(a.borrow_mut().as_mut().unwrap())) }
fn doc() -> Document { web_sys::window().unwrap().document().unwrap() }
fn el(id: &str) -> Element { doc().get_element_by_id(id).unwrap() }
fn html(id: &str, s: &str) { el(id).set_inner_html(s) }
fn val(id: &str) -> String {
    let e = el(id);
    match e.dyn_ref::<HtmlInputElement>() { Some(i) => i.value(), None => e.unchecked_into::<HtmlSelectElement>().value() }
}
fn set_val(id: &str, v: &str) {
    let e = el(id);
    match e.dyn_ref::<HtmlInputElement>() { Some(i) => i.set_value(v), None => e.unchecked_into::<HtmlSelectElement>().set_value(v) }
}
fn num(s: &str) -> f64 { s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0) }
fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
fn tr(en: bool, ko: &str, e: &str) -> String { if en { e.into() } else { ko.into() } }
fn hz(v: f64) -> String { if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.0} Hz") } }

fn on(target: &web_sys::EventTarget, ev: &str, f: impl FnMut(Event) + 'static) {
    let c = Closure::<dyn FnMut(Event)>::new(f);
    target.add_event_listener_with_callback(ev, c.as_ref().unchecked_ref()).unwrap();
    c.forget();
}

#[wasm_bindgen(start)]
pub fn start() {
    APP.with(|a| *a.borrow_mut() = Some(App { bus: presets()[0].1.clone(), msgs: vec![], en: false, blocks: vec![], sel: None }));
    load_preset("humanoid");
    let d: web_sys::EventTarget = doc().into();
    on(&d, "input", |e| handle_input(&e));
    on(&d, "change", |e| {
        let t: Element = e.target().unwrap().unchecked_into();
        if t.id() == "preset" { load_preset(&val("preset")) } else { handle_input(&e) }
    });
    on(&d, "click", |e| {
        let t: Element = e.target().unwrap().unchecked_into();
        let Ok(Some(b)) = t.closest("[data-act]") else { return };
        let i: usize = b.get_attribute("data-i").and_then(|s| s.parse().ok()).unwrap_or(0);
        match b.get_attribute("data-act").unwrap_or_default().as_str() {
            "add" => with(|a| a.msgs.push(Msg { name: tr(a.en, "새 메시지", "New message"), count: 1, bytes: 8,
                rate_hz: 100.0, dir: Dir::Other, color: "#8d9ab3".into() })),
            "dup" => with(|a| { if i < a.msgs.len() { let m = a.msgs[i].clone(); a.msgs.insert(i + 1, m) } }),
            "del" => with(|a| { if i < a.msgs.len() { a.msgs.remove(i); } }),
            "lang" => { with(|a| a.en = !a.en); apply_lang() }
            "csv" => { export_csv(); return }
            "hint" => { el("hint").set_attribute("hidden", "").unwrap(); return }
            "sheet" => { let _ = el("sheet").class_list().toggle("open"); return }
            _ => return,
        }
        render_rows();
        update();
    });
    let cv: web_sys::EventTarget = el("timeline").into();
    on(&cv, "mousemove", |e| {
        let x = e.unchecked_ref::<MouseEvent>().offset_x() as f64;
        let s = with(|a| a.blocks.iter().position(|b| x >= b.x0 && x <= b.x1.max(b.x0 + 2.0)));
        with(|a| a.sel = s);
        draw_timeline();
    });
    on(&cv, "mouseleave", |_| { with(|a| a.sel = None); draw_timeline() });
    on(&cv, "focus", |_| { with(|a| if a.sel.is_none() && !a.blocks.is_empty() { a.sel = Some(0) }); draw_timeline() });
    on(&cv, "blur", |_| { with(|a| a.sel = None); draw_timeline() });
    on(&cv, "keydown", |e| {
        let k = e.unchecked_ref::<KeyboardEvent>().key();
        let moved = with(|a| {
            let n = a.blocks.len();
            if n == 0 { return false }
            a.sel = match (k.as_str(), a.sel) {
                ("ArrowRight", None) | ("Home", _) => Some(0),
                ("ArrowLeft", None) | ("End", _) => Some(n - 1),
                ("ArrowRight", Some(i)) => Some((i + 1).min(n - 1)),
                ("ArrowLeft", Some(i)) => Some(i.saturating_sub(1)),
                ("Escape", _) => None,
                _ => return false,
            };
            true
        });
        if moved { e.prevent_default(); draw_timeline() }
    });
    on(&web_sys::window().unwrap().into(), "resize", |_| update());
}

fn load_preset(key: &str) {
    let Some((_, bus, msgs)) = presets().into_iter().find(|p| p.0 == key) else { return };
    set_val("proto", if bus.proto == Proto::Fd { "fd" } else { "classic" });
    set_val("nominal", &format!("{}", bus.nominal));
    set_val("data", &format!("{}", bus.data.max(1e6)));
    set_val("idtype", if bus.ext_id { "29" } else { "11" });
    set_val("stuff", match bus.stuff { Stuff::None => "none", Stuff::Typical => "typical", Stuff::Worst => "worst" });
    set_val("target", &format!("{}", bus.target_pct));
    with(|a| { a.bus = bus; a.msgs = msgs; a.sel = None });
    render_rows();
    update();
}

fn read_bus(a: &mut App) {
    let proto = if val("proto") == "fd" { Proto::Fd } else { Proto::Classic };
    let nominal = num(&val("nominal")).max(1e3);
    a.bus = Bus { proto, nominal, data: if proto == Proto::Fd { num(&val("data")).max(nominal) } else { nominal },
        ext_id: val("idtype") == "29",
        stuff: match val("stuff").as_str() { "none" => Stuff::None, "typical" => Stuff::Typical, _ => Stuff::Worst },
        target_pct: num(&val("target")).clamp(1.0, 100.0) };
}

fn handle_input(e: &Event) {
    let t: Element = e.target().unwrap().unchecked_into();
    if let Ok(Some(row)) = t.closest("[data-row]") {
        let i: usize = row.get_attribute("data-row").and_then(|s| s.parse().ok()).unwrap_or(0);
        let v = val_of(&t);
        with(|a| {
            let Some(m) = a.msgs.get_mut(i) else { return };
            match t.get_attribute("data-f").unwrap_or_default().as_str() {
                "name" => m.name = v,
                "color" => m.color = v,
                "count" => m.count = num(&v).clamp(0.0, 1000.0) as u32,
                "bytes" => m.bytes = num(&v).clamp(0.0, 64.0) as u32,
                "rate" => m.rate_hz = num(&v).clamp(0.0, 1e5),
                "dir" => m.dir = Dir::from_key(&v),
                _ => {}
            }
        });
    } else if t.id() == "preset" {
        return;
    } else {
        with(read_bus);
    }
    update();
}

fn val_of(t: &Element) -> String {
    match t.dyn_ref::<HtmlInputElement>() { Some(i) => i.value(), None => t.unchecked_ref::<HtmlSelectElement>().value() }
}

fn apply_lang() {
    let en = with(|a| a.en);
    let nodes = doc().query_selector_all("[data-ko]").unwrap();
    for i in 0..nodes.length() {
        let n: Element = nodes.item(i).unwrap().unchecked_into();
        n.set_text_content(n.get_attribute(if en { "data-en" } else { "data-ko" }).as_deref());
    }
    let _ = doc().document_element().unwrap().set_attribute("lang", if en { "en" } else { "ko" });
    doc().query_selector("[data-act=lang]").unwrap().unwrap().set_text_content(Some(if en { "한국어" } else { "EN" }));
    render_rows();
    update();
}

fn render_rows() {
    let mut s = String::new();
    with(|a| {
        let en = a.en;
        for (i, m) in a.msgs.iter().enumerate() {
            let opt = |k: &str, ko: &str, e: &str| format!("<option value=\"{k}\"{}>{}</option>",
                if m.dir.key() == k { " selected" } else { "" }, tr(en, ko, e));
            s += &format!(concat!("<tr data-row=\"{i}\"><td><input type=\"color\" data-f=\"color\" value=\"{c}\" aria-label=\"{lc}\"></td>",
                "<td class=\"l\"><input class=\"name\" data-f=\"name\" value=\"{n}\" aria-label=\"{ln}\"></td>",
                "<td><input type=\"number\" min=\"0\" max=\"1000\" data-f=\"count\" value=\"{cnt}\" aria-label=\"{lcnt}\"></td>",
                "<td><input type=\"number\" min=\"0\" max=\"64\" data-f=\"bytes\" value=\"{b}\" aria-label=\"{lb}\"><span class=\"snap\" id=\"snap{i}\"></span></td>",
                "<td><input type=\"number\" min=\"0\" step=\"any\" data-f=\"rate\" value=\"{r}\" aria-label=\"{lr}\"></td>",
                "<td><select data-f=\"dir\" aria-label=\"{ld}\">{o1}{o2}{o3}</select></td>",
                "<td id=\"b{i}\"></td><td id=\"u{i}\"></td><td id=\"f{i}\"></td><td id=\"l{i}\"></td><td id=\"s{i}\"></td>",
                "<td><button class=\"rowbtn\" data-act=\"dup\" data-i=\"{i}\" aria-label=\"{ldup}\" title=\"{ldup}\">⧉</button>",
                "<button class=\"rowbtn\" data-act=\"del\" data-i=\"{i}\" aria-label=\"{ldel}\" title=\"{ldel}\">✕</button></td></tr>"),
                i = i, c = esc(&m.color), n = esc(&m.name), cnt = m.count, b = m.bytes, r = m.rate_hz,
                o1 = opt("command", "명령", "command"), o2 = opt("feedback", "피드백", "feedback"), o3 = opt("other", "기타", "other"),
                lc = tr(en, "색상", "Colour"), ln = tr(en, "이름", "Name"), lcnt = tr(en, "개수", "Count"),
                lb = tr(en, "페이로드 바이트", "Payload bytes"), lr = tr(en, "주기 Hz", "Rate Hz"), ld = tr(en, "방향", "Direction"),
                ldup = tr(en, "행 복제", "Duplicate row"), ldel = tr(en, "행 삭제", "Delete row"));
        }
    });
    html("rows", &s);
}

fn status_txt(en: bool, s: Status) -> (String, &'static str) {
    match s {
        Status::Pass => (tr(en, "통과", "PASS"), "pass"),
        Status::Warn => (tr(en, "목표 초과", "OVER TARGET"), "warn"),
        Status::Fail => (tr(en, "실패", "FAIL"), "fail"),
    }
}

fn update() {
    let (bus, msgs, en) = with(|a| (a.bus.clone(), a.msgs.clone(), a.en));
    let s = analyze(&bus, &msgs);
    let fd = bus.proto == Proto::Fd;
    for id in ["datafield", "dspfield"] { let _ = el(id).toggle_attribute_with_force("hidden", !fd); }
    for (i, (m, r)) in msgs.iter().zip(&s.rows).enumerate() {
        html(&format!("snap{i}"), &if r.bytes != m.bytes { format!("→{}", r.bytes) } else { String::new() });
        html(&format!("b{i}"), &r.bits.to_string());
        html(&format!("u{i}"), &format!("{:.1}", r.us));
        html(&format!("f{i}"), &format!("{:.0}", r.fps));
        html(&format!("l{i}"), &format!("{:.2}", r.load_pct));
        html(&format!("s{i}"), &format!("{:.1}%", r.share_pct));
    }
    let (st, cls) = status_txt(en, s.status);
    let lc = if s.load_pct > 100.0 { "fail" } else if s.load_pct > bus.target_pct { "warn" } else { "pass" };
    html("total", &format!("<td></td><td class=\"l\">{}</td><td colspan=\"6\"></td><td>{:.0}</td><td class=\"{lc}\">{:.2}</td><td>100%</td><td></td>",
        tr(en, "합계", "Total"), s.fps, s.load_pct));
    let fill = if s.period_us > 0.0 { s.cycle_us / s.period_us * 100.0 } else { 0.0 };
    let fc = if fill > 100.0 { "fail" } else if fill > bus.target_pct { "warn" } else { "pass" };
    let kv = |k: String, v: String, c: &str| format!("<div><span>{k}</span><b class=\"{c}\">{v}</b></div>");
    html("summary", &[
        kv(tr(en, "버스 부하 / 목표", "Bus load / target"), format!("{:.1}% / {:.0}%", s.load_pct, bus.target_pct), lc),
        kv(tr(en, "제어 주기 (가장 빠른 행)", "Control rate (fastest rows)"), format!("{} · {:.0} µs", hz(s.control_hz), s.period_us), ""),
        kv(tr(en, "1주기 전송 시간 / 주기", "One cycle send time / period"), format!("{:.0} / {:.0} µs ({fill:.0}%)", s.cycle_us, s.period_us), fc),
        kv(tr(en, "목표 내 최대 제어 주기", "Max control rate under target"), hz(s.max_hz), if s.max_hz >= s.control_hz { "pass" } else { "warn" }),
        kv(tr(en, "판정", "Status"), st.clone(), cls),
    ].concat());
    html("hud", &format!("<div><span>{}</span><b class=\"{lc}\">{:.1}%</b></div><div><span>{}</span><b class=\"{fc}\">{fill:.0}%</b></div><div><span>{}</span><b>{}</b></div><div><span>{}</span><b class=\"{cls}\">{st}</b></div>",
        tr(en, "버스 부하", "Bus load"), s.load_pct, tr(en, "주기 점유", "Cycle fill"), tr(en, "최대 주기", "Max rate"), hz(s.max_hz), tr(en, "판정", "Status")));
    html("timing", &timing_html(&bus, en));
    layout_blocks(&msgs, &s);
    draw_timeline();
    draw_rate(&bus, &s, en);
}

fn timing_html(bus: &Bus, en: bool) -> String {
    let clock = num(&val("clock")) * 1e6;
    let mut out = String::new();
    let mut phase = |title: String, rate: f64, sp: f64, lim| {
        let list = bit_timings(clock, rate, sp, lim);
        out += &format!("<p><b>{title}</b> · {:.0} kbit/s @ {sp}%</p>", rate / 1e3);
        if list.is_empty() { out += &format!("<p class=\"warn\">{}</p>", tr(en, "±0.5% 이내 조합 없음", "No combination within ±0.5%")); return }
        out += "<div class=\"scroll\"><table><tr><th>BRP</th><th>tq</th><th>TSEG1</th><th>TSEG2</th><th>SJW</th><th>err %</th><th>SP %</th></tr>";
        for t in list.iter().take(6) {
            out += &format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:+.3}</td><td>{:.1}</td></tr>",
                t.prescaler, t.tq, t.tseg1, t.tseg2, t.sjw, t.err_pct, t.sp_pct);
        }
        out += "</table></div>";
    };
    phase(tr(en, "공칭 (중재) 위상", "Nominal (arbitration) phase"), bus.nominal, num(&val("sp")), NOMINAL_LIMITS);
    if bus.proto == Proto::Fd {
        phase(tr(en, "데이터 위상", "Data phase"), bus.data, num(&val("dsp")), DATA_LIMITS);
    }
    out + &format!("<p class=\"note\">{}</p>", tr(en, "TSEG1 = Prop_Seg + Phase_Seg1, 비트 = 1 + TSEG1 + TSEG2 tq. 컨트롤러별 레지스터 값은 보통 1을 뺀 값입니다.",
        "TSEG1 = Prop_Seg + Phase_Seg1, bit = 1 + TSEG1 + TSEG2 tq. Controller registers usually store value − 1."))
}

fn canvas(id: &str) -> (Ctx, f64, f64) {
    let c: HtmlCanvasElement = el(id).unchecked_into();
    let dpr = web_sys::window().unwrap().device_pixel_ratio();
    let (w, h) = (c.client_width() as f64, c.client_height() as f64);
    c.set_width((w * dpr) as u32);
    c.set_height((h * dpr) as u32);
    let ctx: Ctx = c.get_context("2d").unwrap().unwrap().unchecked_into();
    ctx.scale(dpr, dpr).unwrap();
    (ctx, w, h)
}

const PADX: f64 = 14.0;

/// Time axis: one control period, stretched when the cycle (incl. average slower traffic) overflows it.
fn span_us(s: &Summary) -> f64 { s.period_us.max(s.load_pct.min(1e4) / 100.0 * s.period_us).max(1.0) }

fn layout_blocks(msgs: &[Msg], s: &Summary) {
    let w = el("timeline").client_width() as f64 - 2.0 * PADX;
    let k = w / span_us(s);
    let mut x = PADX;
    let mut blocks = vec![];
    for (row, (m, r)) in msgs.iter().zip(&s.rows).enumerate() {
        if r.control {
            for i in 0..m.count { blocks.push(Block { x0: x, x1: x + r.us * k, row, k: i, avg: false }); x += r.us * k; }
        } else if s.control_hz > 0.0 && r.fps > 0.0 {
            let wd = r.fps / s.control_hz * r.us * k;
            blocks.push(Block { x0: x, x1: x + wd, row, k: m.count, avg: true });
            x += wd;
        }
    }
    with(|a| { a.blocks = blocks; if a.sel.is_some_and(|i| i >= a.blocks.len()) { a.sel = None } });
}

fn draw_timeline() {
    let (ctx, w, h) = canvas("timeline");
    let (bus, msgs, en, sel) = with(|a| (a.bus.clone(), a.msgs.clone(), a.en, a.sel));
    let s = analyze(&bus, &msgs);
    let span = span_us(&s);
    let k = (w - 2.0 * PADX) / span;
    let (y0, y1) = (62.0, h - 52.0);
    let xp = PADX + s.period_us * k;
    ctx.set_font("12px system-ui, sans-serif");
    ctx.set_fill_style_str("#8d9ab3");
    let _ = ctx.fill_text(&format!("{} · {} · {:.0} µs", tr(en, "제어 1주기 버스 타임라인", "One control cycle on the bus"), hz(s.control_hz), s.period_us), PADX, 22.0);
    // idle / overflow region
    let used = with(|a| a.blocks.last().map(|b| b.x1).unwrap_or(PADX));
    ctx.set_fill_style_str("#1a2335");
    ctx.fill_rect(PADX, y0, (w - 2.0 * PADX).max(0.0), y1 - y0);
    if used < xp {
        ctx.set_fill_style_str("#8d9ab3");
        ctx.set_text_align("center");
        let _ = ctx.fill_text(&format!("{} {:.0} µs", tr(en, "유휴", "idle"), (xp - used) / k), (used + xp) / 2.0, (y0 + y1) / 2.0 + 4.0);
        ctx.set_text_align("start");
    } else {
        ctx.set_fill_style_str("#ff6b6b33");
        ctx.fill_rect(xp, y0 - 6.0, used - xp, y1 - y0 + 12.0);
    }
    with(|a| for (i, b) in a.blocks.iter().enumerate() {
        ctx.set_global_alpha(if b.avg { 0.4 } else { 1.0 });
        ctx.set_fill_style_str(&msgs[b.row].color);
        ctx.fill_rect(b.x0, y0, (b.x1 - b.x0).max(0.5), y1 - y0);
        if b.x1 - b.x0 > 3.0 { ctx.set_fill_style_str("#0e1522"); ctx.fill_rect(b.x1 - 0.75, y0, 0.75, y1 - y0) }
        ctx.set_global_alpha(1.0);
        if sel == Some(i) {
            ctx.set_stroke_style_str("#ffffff");
            ctx.set_line_width(2.0);
            ctx.stroke_rect(b.x0, y0 - 3.0, (b.x1 - b.x0).max(2.0), y1 - y0 + 6.0);
        }
    });
    // period end and target line
    ctx.set_stroke_style_str("#e6ecf5");
    ctx.set_line_width(1.5);
    ctx.begin_path(); ctx.move_to(xp, y0 - 10.0); ctx.line_to(xp, y1 + 10.0); ctx.stroke();
    let xt = PADX + s.period_us * bus.target_pct / 100.0 * k;
    ctx.set_stroke_style_str("#f2b84b");
    let _ = ctx.set_line_dash(&js_sys::Array::of2(&5.into(), &4.into()));
    ctx.begin_path(); ctx.move_to(xt, y0 - 14.0); ctx.line_to(xt, y1 + 10.0); ctx.stroke();
    let _ = ctx.set_line_dash(&js_sys::Array::new());
    ctx.set_fill_style_str("#f2b84b");
    let _ = ctx.fill_text(&format!("{} {:.0}%", tr(en, "목표", "target"), bus.target_pct), xt + 4.0, y0 - 6.0);
    // axis
    ctx.set_fill_style_str("#8d9ab3");
    ctx.set_stroke_style_str("#26324a");
    ctx.set_line_width(1.0);
    let nt = ((w / 120.0) as u32).clamp(2, 5);
    for i in 0..=nt {
        let t = span * i as f64 / nt as f64;
        let x = PADX + t * k;
        ctx.begin_path(); ctx.move_to(x, y1 + 12.0); ctx.line_to(x, y1 + 18.0); ctx.stroke();
        ctx.set_text_align(if i == 0 { "start" } else if i == nt { "end" } else { "center" });
        let _ = ctx.fill_text(&format!("{t:.0} µs"), x, y1 + 32.0);
    }
    ctx.set_text_align("start");
    // tooltip
    let tip: HtmlElement = el("tip").unchecked_into();
    let info = with(|a| sel.and_then(|i| a.blocks.get(i)).map(|b| (b.x0, b.x1, b.row, b.k, b.avg)));
    match info {
        Some((x0, x1, row, kk, avg)) => {
            let (m, r) = (&msgs[row], &s.rows[row]);
            let what = if avg { format!("{} ({}× @ {})", tr(en, "주기당 평균", "avg per cycle"), kk, hz(m.rate_hz)) }
                else { format!("#{} / {}", kk + 1, m.count) };
            tip.set_text_content(Some(&format!("{} — {what}\n{} B · {} bits · {:.1} µs\n{}: {:.2}% · {}: {:.1}%", m.name, r.bytes, r.bits, r.us,
                tr(en, "부하", "load"), r.load_pct, tr(en, "점유율", "share"), r.share_pct)));
            let tw = tip.offset_width() as f64;
            let st = tip.style();
            let _ = st.set_property("left", &format!("{}px", ((x0 + x1) / 2.0 - tw / 2.0).clamp(4.0, (w - tw - 4.0).max(4.0))));
            let _ = st.set_property("top", &format!("{}px", y0 + 6.0));
            let _ = tip.class_list().add_1("on");
        }
        None => { let _ = tip.class_list().remove_1("on"); }
    }
}

fn draw_rate(bus: &Bus, s: &Summary, en: bool) {
    let (ctx, w, h) = canvas("rateplot");
    let (l, r, t, b) = (44.0, 14.0, 12.0, 26.0);
    let xmax = (s.control_hz * 2.0).max(s.max_hz * 1.25).max(10.0);
    let ymax = 120.0;
    let px = |f: f64| l + f / xmax * (w - l - r);
    let py = |p: f64| h - b - p.min(ymax) / ymax * (h - t - b);
    ctx.set_font("11px system-ui, sans-serif");
    ctx.set_stroke_style_str("#26324a");
    ctx.set_fill_style_str("#8d9ab3");
    for i in 0..=4 {
        let p = 30.0 * i as f64;
        ctx.begin_path(); ctx.move_to(l, py(p)); ctx.line_to(w - r, py(p)); ctx.stroke();
        let _ = ctx.fill_text(&format!("{p:.0}%"), 6.0, py(p) + 4.0);
        let f = xmax * i as f64 / 4.0;
        ctx.set_text_align("center");
        let _ = ctx.fill_text(&hz(f), px(f).clamp(l + 12.0, w - r - 20.0), h - 8.0);
        ctx.set_text_align("start");
    }
    ctx.set_stroke_style_str("#f2b84b");
    let _ = ctx.set_line_dash(&js_sys::Array::of2(&5.into(), &4.into()));
    ctx.begin_path(); ctx.move_to(l, py(bus.target_pct)); ctx.line_to(w - r, py(bus.target_pct)); ctx.stroke();
    if s.max_hz > 0.0 { ctx.begin_path(); ctx.move_to(px(s.max_hz), t); ctx.line_to(px(s.max_hz), h - b); ctx.stroke(); }
    let _ = ctx.set_line_dash(&js_sys::Array::new());
    ctx.set_stroke_style_str("#5b9cff");
    ctx.set_line_width(2.0);
    let f_at = |p: f64| if s.per_hz_pct > 0.0 { (p - s.other_pct) / s.per_hz_pct } else { xmax };
    let fend = f_at(ymax).clamp(0.0, xmax);
    ctx.begin_path(); ctx.move_to(px(0.0), py(s.other_pct)); ctx.line_to(px(fend), py(s.other_pct + s.per_hz_pct * fend)); ctx.stroke();
    let (cx, cy) = (px(s.control_hz), py(s.load_pct));
    ctx.set_fill_style_str(if s.load_pct > bus.target_pct { "#ff6b6b" } else { "#3ad1a4" });
    ctx.begin_path(); let _ = ctx.arc(cx, cy, 5.0, 0.0, std::f64::consts::TAU); ctx.fill();
    ctx.set_fill_style_str("#e6ecf5");
    let _ = ctx.fill_text(&format!("{} {} · {:.1}%", tr(en, "현재", "now"), hz(s.control_hz), s.load_pct), (cx + 8.0).min(w - 150.0), (cy - 8.0).max(t + 10.0));
    ctx.set_fill_style_str("#f2b84b");
    let _ = ctx.fill_text(&format!("{} {}", tr(en, "최대", "max"), hz(s.max_hz)), (px(s.max_hz) + 4.0).min(w - 90.0), h - b - 6.0);
}

fn export_csv() {
    let csv = with(|a| to_csv(&a.bus, &a.msgs));
    let parts = js_sys::Array::of1(&JsValue::from_str(&format!("\u{feff}{csv}")));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/csv;charset=utf-8");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let a: web_sys::HtmlAnchorElement = doc().create_element("a").unwrap().unchecked_into();
    a.set_href(&url);
    a.set_download("can-load.csv");
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
}
