//! Gantt renderer: port of Ruby `Parser::Native::Builders::Gantt`,
//! `Notation::Mermaid::IRAdapters::Gantt`, `Layout::Gantt*` and `Renderer::Gantt`.
//!
//! Anything that would read the system clock (`today`), the machine time zone,
//! or a Ruby `Date._parse` / `strftime` shape this port does not reproduce
//! returns `Error::Unported`, so the caller falls back to the Ruby renderer.
use super::pie::{is_slice, items, strip};
use crate::svg::{Document, Elem, Theme};
use crate::Error;
use serde_json::Value;
use std::collections::HashMap;

fn unported() -> Error {
    Error::Unported("gantt".into())
}

// ---- calendar helpers (UTC, proleptic Gregorian) ----

const DAY: i64 = 86_400;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_days(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if leap(y) { 29 } else { 28 },
    }
}

struct Tm {
    year: i64,
    mon: i64,
    day: i64,
    hour: i64,
    min: i64,
    sec: i64,
    wday: i64,
    yday: i64,
}

fn tm(t: i64) -> Tm {
    let days = t.div_euclid(DAY);
    let rem = t.rem_euclid(DAY);
    let (year, mon, day) = civil_from_days(days);
    Tm {
        year,
        mon,
        day,
        hour: rem / 3600,
        min: rem % 3600 / 60,
        sec: rem % 60,
        wday: (days + 4).rem_euclid(7),
        yday: days - days_from_civil(year, 1, 1) + 1,
    }
}

fn utc(y: i64, m: i64, d: i64, h: i64) -> i64 {
    days_from_civil(y, m, d) * DAY + h * 3600
}

const DAY_NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

/// Ruby `Time#strftime` for the directives this port reproduces; anything else is Unported.
fn strftime(t: i64, fmt: &str) -> Result<String, Error> {
    let tm = tm(t);
    if !(1000..=9999).contains(&tm.year) {
        return Err(unported());
    }
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut nopad = false;
        if chars.peek() == Some(&'-') {
            nopad = true;
            chars.next();
        }
        let d = chars.next().ok_or_else(unported)?;
        let two = |v: i64| if nopad { v.to_string() } else { format!("{v:02}") };
        match d {
            'Y' if !nopad => out.push_str(&tm.year.to_string()),
            'y' => out.push_str(&two(tm.year % 100)),
            'm' => out.push_str(&two(tm.mon)),
            'd' => out.push_str(&two(tm.day)),
            'e' => out.push_str(&if nopad { tm.day.to_string() } else { format!("{:2}", tm.day) }),
            'H' => out.push_str(&two(tm.hour)),
            'M' => out.push_str(&two(tm.min)),
            'S' => out.push_str(&two(tm.sec)),
            'j' if !nopad => out.push_str(&format!("{:03}", tm.yday)),
            'A' if !nopad => out.push_str(DAY_NAMES[tm.wday as usize]),
            'a' if !nopad => out.push_str(&DAY_NAMES[tm.wday as usize][..3]),
            'B' if !nopad => out.push_str(MONTH_NAMES[(tm.mon - 1) as usize]),
            'b' if !nopad => out.push_str(&MONTH_NAMES[(tm.mon - 1) as usize][..3]),
            'F' if !nopad => out.push_str(&format!("{}-{:02}-{:02}", tm.year, tm.mon, tm.day)),
            '%' if !nopad => out.push('%'),
            _ => return Err(unported()),
        }
    }
    Ok(out)
}

/// `parse_date` for the one shape this port reproduces: `YYYY-MM-DD`. Returns midnight UTC.
/// Every other input falls back to `today` or `Date._parse` in Ruby: Unported.
fn parse_date(text: &str, fallback: Option<i64>) -> Result<i64, Error> {
    if text.contains('\\') {
        return fallback.ok_or_else(unported);
    }
    let b = text.as_bytes();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !digits(&b[..4]) || !digits(&b[5..7]) || !digits(&b[8..]) {
        return Err(unported());
    }
    let n = |s: &str| s.parse::<i64>().unwrap_or(0);
    let (y, m, d) = (n(&text[..4]), n(&text[5..7]), n(&text[8..]));
    if !(1..=12).contains(&m) || d < 1 || d > month_days(y, m) || y < 1000 {
        return Err(unported());
    }
    Ok(days_from_civil(y, m, d) * DAY)
}

fn iso_date_text(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

// ---- GanttDuration ----

fn duration_add(time: i64, text: &str) -> Result<i64, Error> {
    let t = strip(text);
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return Ok(time);
    }
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == i + 1 {
            return Ok(time);
        }
        i = j;
    }
    let unit = &t[i..];
    if !matches!(unit, "" | "ms" | "M" | "d" | "h" | "m" | "s" | "w" | "y") {
        return Ok(time);
    }
    let value: f64 = t[..i].parse().map_err(|_| unported())?;
    if !(value.is_finite() && value < 1e9) {
        return Err(unported());
    }
    let seconds = match unit {
        "M" | "y" | "ms" => return Err(unported()),
        "d" | "" => value.round() * DAY as f64,
        "w" => (value * 7.0).round() * DAY as f64,
        "h" => value * 3600.0,
        "m" => value * 60.0,
        _ => value,
    };
    if seconds.fract() != 0.0 {
        return Err(unported());
    }
    Ok(time + seconds as i64)
}

// ---- GanttExclusions ----

struct Exclusions {
    tokens: Vec<String>,
    weekend_start: i64,
    date_format: String,
}

impl Exclusions {
    fn new(entries: &[String], weekend: Option<&str>, date_format: &str) -> Self {
        let tokens = entries
            .iter()
            .flat_map(|e| e.to_lowercase().split(|c: char| c.is_whitespace() || c == ',').map(String::from).collect::<Vec<_>>())
            .filter(|t| !t.is_empty())
            .collect();
        let weekend_start = match strip(weekend.unwrap_or("")) {
            "friday" => 5,
            _ => 6,
        };
        let mut fmt = String::new();
        let df = strip(date_format);
        let mut rest = df;
        'outer: while !rest.is_empty() {
            for (tok, rep) in [("YYYY", "%Y"), ("YY", "%y"), ("MM", "%m"), ("DD", "%d"), ("M", "%-m"), ("D", "%-d")] {
                if let Some(r) = rest.strip_prefix(tok) {
                    fmt.push_str(rep);
                    rest = r;
                    continue 'outer;
                }
            }
            let c = rest.chars().next().unwrap();
            fmt.push(c);
            rest = &rest[c.len_utf8()..];
        }
        Exclusions { tokens, weekend_start, date_format: fmt }
    }

    /// `settle`: (pushed end, end to draw the bar to).
    fn settle(&self, start: i64, stop: Option<i64>) -> Result<(Option<i64>, Option<i64>), Error> {
        if self.tokens.is_empty() {
            return Ok((stop, None));
        }
        let Some(mut stop) = stop else { return Err(unported()) };
        let limit = stop + 10_000 * DAY;
        let mut day = start + DAY;
        let (mut drawn, mut pushed) = (None, false);
        while stop >= day && stop <= limit {
            if !pushed {
                drawn = Some(stop);
            }
            pushed = self.excluded(day)?;
            if pushed {
                stop += DAY;
            }
            day += DAY;
        }
        Ok((Some(stop), drawn))
    }

    fn excluded(&self, day: i64) -> Result<bool, Error> {
        if self.weekend(day) {
            return Ok(true);
        }
        let name = DAY_NAMES[tm(day).wday as usize].to_lowercase();
        if self.tokens.contains(&name) {
            return Ok(true);
        }
        for text in [strftime(day, "%Y-%m-%d")?, strftime(day, &self.date_format)?] {
            if self.tokens.contains(&text.to_lowercase()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn weekend(&self, day: i64) -> bool {
        if !self.tokens.iter().any(|t| t == "weekends") {
            return false;
        }
        let w = tm(day).wday;
        let iso = if w == 0 { 7 } else { w };
        iso == self.weekend_start || iso == self.weekend_start + 1
    }
}

// ---- GanttTicks ----

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

const HOUR: i64 = 3600;
const LADDER: [(Unit, i64, i64); 10] = [
    (Unit::Hour, 1, HOUR),
    (Unit::Hour, 3, 3 * HOUR),
    (Unit::Hour, 6, 6 * HOUR),
    (Unit::Hour, 12, 12 * HOUR),
    (Unit::Day, 1, DAY),
    (Unit::Day, 2, 2 * DAY),
    (Unit::Week, 1, 7 * DAY),
    (Unit::Month, 1, 30 * DAY),
    (Unit::Month, 3, 90 * DAY),
    (Unit::Year, 1, 365 * DAY),
];

fn unit_seconds(u: Unit) -> i64 {
    match u {
        Unit::Minute => 60,
        Unit::Hour => HOUR,
        Unit::Day => DAY,
        Unit::Week => 7 * DAY,
        Unit::Month => 30 * DAY,
        Unit::Year => 365 * DAY,
    }
}

fn parse_interval(text: Option<&str>) -> Result<Option<(Unit, i64)>, Error> {
    let t = strip(text.unwrap_or(""));
    let re = regex::Regex::new(r"^([1-9][0-9]*)(minute|hour|day|week|month)$").unwrap();
    let Some(m) = re.captures(t) else { return Ok(None) };
    let step: i64 = m[1].parse().map_err(|_| unported())?;
    let unit = match &m[2] {
        "minute" => Unit::Minute,
        "hour" => Unit::Hour,
        "day" => Unit::Day,
        "week" => Unit::Week,
        _ => Unit::Month,
    };
    Ok(Some((unit, step)))
}

fn nice_step(raw: f64) -> i64 {
    let power = crate::js_math::log10(raw).floor();
    let error = raw / crate::js_math::pow(10.0, power);
    let factor = [(50f64.sqrt(), 10i64), (10f64.sqrt(), 5), (2f64.sqrt(), 2)]
        .iter()
        .find(|(limit, _)| error >= *limit)
        .map(|(_, f)| *f);
    (factor.unwrap_or(1) * 10i64.pow(power as u32)).max(1)
}

fn pick_interval(span: f64) -> (Unit, i64) {
    let target = span.abs() / 10.0;
    match LADDER.iter().position(|e| e.2 as f64 > target) {
        None => (Unit::Year, nice_step(span.abs() / (365 * DAY) as f64 / 10.0)),
        Some(0) => (LADDER[0].0, LADDER[0].1),
        Some(i) => {
            let (below, above) = (LADDER[i - 1], LADDER[i]);
            let pick = if target / (below.2 as f64) < (above.2 as f64) / target { below } else { above };
            (pick.0, pick.1)
        }
    }
}

fn floor_to(unit: Unit, t: i64) -> i64 {
    let c = tm(t);
    match unit {
        Unit::Minute => utc(c.year, c.mon, c.day, c.hour) + c.min * 60,
        Unit::Hour => utc(c.year, c.mon, c.day, c.hour),
        Unit::Month => utc(c.year, c.mon, 1, 0),
        Unit::Year => utc(c.year, 1, 1, 0),
        _ => {
            let day = utc(c.year, c.mon, c.day, 0);
            if unit == Unit::Week { day - c.wday * DAY } else { day }
        }
    }
}

fn advance(unit: Unit, t: i64) -> i64 {
    let c = tm(t);
    match unit {
        Unit::Minute => t + 60,
        Unit::Hour => t + HOUR,
        Unit::Day => t + DAY,
        Unit::Week => t + 7 * DAY,
        Unit::Month => utc(c.year + c.mon / 12, c.mon % 12 + 1, 1, 0),
        Unit::Year => utc(c.year + 1, 1, 1, 0),
    }
}

fn index_of(unit: Unit, t: i64) -> i64 {
    let c = tm(t);
    match unit {
        Unit::Minute => c.min,
        Unit::Hour => c.hour,
        Unit::Day => c.day - 1,
        Unit::Week => (t.div_euclid(DAY) + 4).div_euclid(7),
        Unit::Month => (c.year - 1970) * 12 + c.mon - 1,
        Unit::Year => c.year,
    }
}

/// Axis tick times between `start` and `start + span` (`stop` may carry a float part).
fn ticks(start: i64, span: f64, interval: Option<&str>) -> Result<Vec<i64>, Error> {
    let explicit = parse_interval(interval)?
        .filter(|(u, step)| span.abs() / (unit_seconds(*u) as f64 * *step as f64) <= 2000.0);
    let (unit, step) = explicit.unwrap_or_else(|| pick_interval(span));
    let within = |t: i64| t >= start && ((t - start) as f64) <= span;
    let mut cursor = floor_to(unit, start);
    let mut found = vec![];
    while ((cursor - start) as f64) <= span {
        found.push(cursor);
        if found.len() > 200_000 {
            return Err(unported());
        }
        cursor = advance(unit, cursor);
    }
    Ok(found.into_iter().filter(|t| within(*t) && index_of(unit, *t).rem_euclid(step) == 0).collect())
}

// ---- parse tree -> diagram (Builders::Gantt) ----

#[derive(Default, Clone)]
struct Task {
    description: String,
    id: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    duration: Option<String>,
    after_task: Option<String>,
    until_task: Option<String>,
    tags: Vec<String>,
    calc_start: Option<i64>,
    calc_end: Option<i64>,
    render_end: Option<i64>,
}

struct Section {
    name: String,
    tasks: Vec<Task>,
}

struct Diagram {
    title: Option<String>,
    date_format: String,
    axis_format: Option<String>,
    tick_interval: Option<String>,
    excludes: Vec<String>,
    weekend: Option<String>,
    sections: Vec<Section>,
}

/// Builder `extract_text`.
fn extract(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        Value::String(s) => Ok(strip(s).to_string()),
        v if is_slice(v) => Ok(strip(v["value"].as_str().unwrap_or("")).to_string()),
        _ => Err(unported()),
    }
}

fn duration_shaped(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == i + 1 {
            return false;
        }
        i = j;
    }
    matches!(&s[i..], "ms" | "M" | "d" | "h" | "m" | "s" | "w" | "y")
}

fn dependency(field: &str, keyword: &str) -> Option<String> {
    let rest = field.strip_prefix(keyword)?;
    let t = rest.trim_start_matches([' ', '\t', '\r', '\n', '\u{b}', '\u{c}']);
    (t.len() < rest.len() && !t.is_empty() && !t.contains('\n')).then(|| t.to_string())
}

fn build_task(entry: &Value) -> Result<Task, Error> {
    let mut task = Task { description: extract(entry.get("description").ok_or_else(unported)?)?, ..Task::default() };
    let Some(details) = entry.get("task_details").filter(|d| !d.is_null()) else { return Ok(task) };
    let parts: Vec<&Value> = match details {
        Value::Array(a) => a.iter().collect(),
        d => vec![d],
    };
    let mut fields = vec![];
    for p in parts {
        fields.push(extract(p.get("field").ok_or_else(unported)?)?);
    }
    let mut positional = vec![];
    for f in fields {
        if matches!(f.as_str(), "done" | "active" | "crit" | "milestone") {
            task.tags.push(f);
        } else {
            positional.push(f);
        }
    }
    if positional.len() == 3 {
        task.id = Some(positional.remove(0));
    }
    let mut values = vec![];
    for f in positional {
        if let Some(a) = dependency(&f, "after") {
            task.after_task = Some(a);
        } else if let Some(u) = dependency(&f, "until") {
            task.until_task = Some(u);
        } else {
            values.push(f);
        }
    }
    let mut dates = vec![];
    for f in values {
        if duration_shaped(&f) {
            task.duration = Some(f);
        } else {
            dates.push(f);
        }
    }
    if dates.len() == 1 && task.after_task.is_some() {
        task.end_date = Some(dates.remove(0));
    } else {
        let mut it = dates.into_iter();
        task.start_date = it.next();
        if let Some(e) = it.next() {
            task.end_date = Some(e);
        }
    }
    Ok(task)
}

fn build(tree: &Value) -> Result<Diagram, Error> {
    let mut d = Diagram {
        title: None,
        date_format: "YYYY-MM-DD".into(),
        axis_format: None,
        tick_interval: None,
        excludes: vec![],
        weekend: None,
        sections: vec![],
    };
    for item in items(tree) {
        if let Some(v) = item.get("title") {
            d.title = Some(extract(v)?);
        }
        if let Some(v) = item.get("date_format") {
            d.date_format = extract(v)?;
        }
        if let Some(v) = item.get("axis_format") {
            d.axis_format = Some(extract(v)?);
        }
        if let Some(v) = item.get("tick_interval") {
            d.tick_interval = Some(extract(v)?);
        }
        if let Some(v) = item.get("excludes") {
            let t = extract(v)?;
            if !t.is_empty() {
                d.excludes.push(t);
            }
        }
        if let Some(v) = item.get("weekend") {
            d.weekend = Some(extract(v)?);
        }
        for key in ["today_marker", "acc_title", "acc_descr", "click_id"] {
            if let Some(v) = item.get(key) {
                extract(v)?;
            }
        }
        if let Some(v) = item.get("section") {
            d.sections.push(Section { name: extract(v)?, tasks: vec![] });
        }
        if let Some(entry) = item.get("task_entry") {
            if d.sections.is_empty() {
                d.sections.push(Section { name: "Default".into(), tasks: vec![] });
            }
            let task = build_task(entry)?;
            d.sections.last_mut().unwrap().tasks.push(task);
        }
    }
    Ok(d)
}

// ---- layout: dates ----

struct Schedule<'a> {
    ids: HashMap<String, (usize, usize)>,
    ex: Exclusions,
    d: &'a mut Diagram,
}

impl Schedule<'_> {
    fn task(&self, at: (usize, usize)) -> &Task {
        &self.d.sections[at.0].tasks[at.1]
    }

    fn task_mut(&mut self, at: (usize, usize)) -> &mut Task {
        &mut self.d.sections[at.0].tasks[at.1]
    }

    fn settle_end(&mut self, at: (usize, usize), stop: Option<i64>) -> Result<(), Error> {
        let t = self.task(at);
        if t.end_date.as_deref().is_some_and(iso_date_text) {
            let t = self.task_mut(at);
            t.calc_end = stop;
            t.render_end = None;
            return Ok(());
        }
        let start = t.calc_start.ok_or_else(unported)?;
        let (end, drawn) = self.ex.settle(start, stop)?;
        let t = self.task_mut(at);
        t.calc_end = end;
        t.render_end = drawn;
        Ok(())
    }

    fn explicit_end(&self, at: (usize, usize)) -> Result<i64, Error> {
        let t = self.task(at);
        parse_date(t.end_date.as_deref().ok_or_else(unported)?, t.calc_start)
    }

    fn end_for(&self, at: (usize, usize)) -> Result<i64, Error> {
        let t = self.task(at);
        duration_add(t.calc_start.ok_or_else(unported)?, t.duration.as_deref().unwrap_or(""))
    }

    fn calculate_task_date(&mut self, at: (usize, usize)) -> Result<(), Error> {
        let start = parse_date(self.task(at).start_date.as_deref().ok_or_else(unported)?, None)?;
        self.task_mut(at).calc_start = Some(start);
        let t = self.task(at);
        if t.end_date.is_some() {
            let end = self.explicit_end(at)?;
            self.settle_end(at, Some(end))
        } else if t.duration.is_some() {
            let end = self.end_for(at)?;
            self.settle_end(at, Some(end))
        } else {
            Ok(())
        }
    }

    fn unresolved(&self, at: (usize, usize)) -> bool {
        let t = self.task(at);
        if t.calc_start.is_none() {
            return true;
        }
        t.until_task.is_some() && t.start_date.is_some() && t.after_task.is_none() && t.calc_end.is_none()
    }

    fn latest_dependency(&self, names: &str) -> Option<i64> {
        let mut latest: Option<i64> = None;
        for id in names.split_whitespace() {
            let end = self.ids.get(id).and_then(|at| self.task(*at).calc_end)?;
            latest = Some(latest.map_or(end, |l| l.max(end)));
        }
        latest
    }

    fn resolve_after(&mut self, at: (usize, usize)) -> Result<bool, Error> {
        let names = self.task(at).after_task.clone().unwrap();
        // `names` always holds at least one id (the builder requires `.+`).
        let Some(end) = self.latest_dependency(&names) else { return Ok(false) };
        self.task_mut(at).calc_start = Some(end);
        let t = self.task(at);
        if t.duration.is_some() {
            let e = self.end_for(at)?;
            self.settle_end(at, Some(e))?;
        } else if t.end_date.is_some() {
            let e = self.explicit_end(at)?;
            self.settle_end(at, Some(e))?;
        } else if let Some(u) = t.until_task.clone() {
            if let Some(ut) = self.ids.get(&u).copied() {
                let stop = self.task(ut).calc_start;
                self.settle_end(at, stop)?;
            }
        }
        Ok(true)
    }

    fn resolve_until(&mut self, at: (usize, usize)) -> Result<bool, Error> {
        let t = self.task(at);
        let (Some(u), Some(sd)) = (t.until_task.clone(), t.start_date.clone()) else { return Ok(false) };
        let start = parse_date(&sd, None)?;
        self.task_mut(at).calc_start = Some(start);
        let Some(ut) = self.ids.get(&u).copied() else { return Ok(false) };
        let Some(stop) = self.task(ut).calc_start else { return Ok(false) };
        self.settle_end(at, Some(stop))?;
        Ok(true)
    }

    fn resolve_unsettled(&mut self, at: (usize, usize), prev: Option<(usize, usize)>) -> Result<bool, Error> {
        if !self.unresolved(at) {
            return Ok(false);
        }
        let t = self.task(at);
        if t.after_task.is_some() {
            return self.resolve_after(at);
        }
        if t.until_task.is_some() {
            return self.resolve_until(at);
        }
        if t.duration.is_none() {
            return Ok(false);
        }
        let Some(prev) = prev else { return Ok(false) };
        let Some(prev_end) = self.task(prev).calc_end else { return Ok(false) };
        self.task_mut(at).calc_start = Some(prev_end);
        let e = self.end_for(at)?;
        self.settle_end(at, Some(e))?;
        Ok(true)
    }
}

fn calculate_dates(d: &mut Diagram) -> Result<(), Error> {
    let order: Vec<(usize, usize)> =
        d.sections.iter().enumerate().flat_map(|(s, sec)| (0..sec.tasks.len()).map(move |t| (s, t))).collect();
    let mut ids = HashMap::new();
    for at in &order {
        if let Some(id) = &d.sections[at.0].tasks[at.1].id {
            ids.insert(id.clone(), *at);
        }
    }
    let ex = Exclusions::new(&d.excludes, d.weekend.as_deref(), &d.date_format);
    let mut s = Schedule { ids, ex, d };
    for at in &order {
        let t = s.task(*at);
        if t.start_date.is_some() && t.after_task.is_none() {
            s.calculate_task_date(*at)?;
        }
    }
    for _ in 0..100 {
        let mut changed = false;
        for (i, at) in order.iter().enumerate() {
            let prev = if i > 0 { Some(order[i - 1]) } else { None };
            if s.resolve_unsettled(*at, prev)? {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

// ---- layout: geometry ----

const MARGIN_LEFT: f64 = 200.0;
const MARGIN_TOP: f64 = 80.0;
const MARGIN_RIGHT: f64 = 50.0;
const MARGIN_BOTTOM: f64 = 50.0;
const ROW_HEIGHT: i64 = 24;
const TASK_BAR_HEIGHT: f64 = 20.0;
const TIMELINE_WIDTH: f64 = 800.0;
const PLOT_WIDTH: f64 = 634.0;
const TIMELINE_HEIGHT: f64 = 40.0;
const TITLE_Y: f64 = 40.0;

struct Axis {
    origin: i64,
    days: f64,
    width: f64,
}

struct Timeline {
    start: i64,
    total_days: f64,
}

fn task_axis(tl: &Timeline) -> Axis {
    let task_days = tl.total_days - 2.0;
    if !(task_days > 0.0) {
        return Axis { origin: tl.start, days: tl.total_days, width: TIMELINE_WIDTH };
    }
    Axis { origin: tl.start + DAY, days: task_days, width: PLOT_WIDTH }
}

/// [tick time, fraction of the timeline] pairs (`tick_positions`).
fn tick_positions(tl: &Timeline, interval: Option<&str>) -> Result<Vec<(i64, f64)>, Error> {
    let ax = task_axis(tl);
    let span_all = ax.days * DAY as f64;
    let (start, span) =
        if ax.days < tl.total_days { (ax.origin, span_all) } else { (ax.origin + DAY, span_all - 2.0 * DAY as f64) };
    if ax.days <= 0.0 || !ax.days.is_finite() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for tick in ticks(start, span, interval)? {
        let fraction = (tick - ax.origin) as f64 / span_all;
        let pixels = (fraction * ax.width).round() + 0.5;
        out.push((tick, pixels / TIMELINE_WIDTH));
    }
    Ok(out)
}

struct Fonts {
    large: f64,
    normal: f64,
    small: f64,
    family: &'static str,
}

fn text(content: &str, x: f64, y: f64, size: f64, fonts: &Fonts, theme: Theme) -> Elem {
    Elem::new("text")
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", x)
        .num("y", y)
        .attr("font-family", fonts.family)
        .attr("font-size", crate::svg::ruby_float(size))
        .content(content)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Elem {
    Elem::new("rect").num("x", x).num("y", y).num("width", w).num("height", h)
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "gantt")?;
    let mut d = build(&tree)?;
    calculate_dates(&mut d)?;
    let theme = Theme::default_theme();
    let fonts = Fonts {
        large: theme.number("typography", "font_size_large").ok_or_else(unported)?,
        normal: theme.number("typography", "font_size_normal").ok_or_else(unported)?,
        small: theme.number("typography", "font_size_small").ok_or_else(unported)?,
        family: theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"),
    };
    let color = |k: &str| theme.color(k).unwrap_or("");

    // Timeline bounds; `today` is only read when it would change the output.
    let all: Vec<&Task> = d.sections.iter().flat_map(|s| s.tasks.iter()).collect();
    let task_count = all.len();
    let min = all.iter().filter_map(|t| t.calc_start).min();
    let max = all.iter().filter_map(|t| t.calc_end).max();
    let tl = match (min, max) {
        (Some(min), Some(max)) => {
            let (start, end) = (min - DAY, max + DAY);
            Some(Timeline { start, total_days: (end - start) as f64 / DAY as f64 })
        }
        _ if task_count == 0 => None,
        _ => return Err(unported()),
    };
    if let Some(tl) = &tl {
        if tl.total_days <= 0.0 {
            return Err(unported());
        }
    }

    let rows = task_count as i64;
    let extent = MARGIN_TOP + TIMELINE_HEIGHT + (rows * ROW_HEIGHT) as f64;
    let width = MARGIN_LEFT + TIMELINE_WIDTH + MARGIN_RIGHT;
    let height = extent + MARGIN_BOTTOM;
    let mut doc = Document::new(width, height);
    doc.view_box = Some(format!("0 0 {} {}", width as i64, height as i64));

    if let Some(title) = &d.title {
        doc.add(
            text(title, MARGIN_LEFT + (TIMELINE_WIDTH / 2.0).floor(), TITLE_Y, fonts.large, &fonts, theme)
                .attr("text-anchor", "middle")
                .attr("font-weight", "bold"),
        );
    }

    if task_count > 0 {
        let tl = tl.as_ref().unwrap();
        doc.add(
            rect(MARGIN_LEFT, MARGIN_TOP, TIMELINE_WIDTH, TIMELINE_HEIGHT)
                .attr("fill", color("node_fill"))
                .attr("stroke", color("node_stroke"))
                .attr("stroke-width", "1"),
        );
        let positions = tick_positions(tl, d.tick_interval.as_deref())?;
        for (tick, pos) in &positions {
            let label = match &d.axis_format {
                None => strftime(*tick, "%Y-%m-%d")?,
                Some(f) => strftime(*tick, f)?,
            };
            doc.add(
                text(&label, MARGIN_LEFT + pos * TIMELINE_WIDTH, extent + 5.0, fonts.small, &fonts, theme)
                    .attr("text-anchor", "middle"),
            );
        }
        for (_, pos) in &positions {
            let x = MARGIN_LEFT + pos * TIMELINE_WIDTH;
            doc.add(
                Elem::new("line")
                    .attr("stroke", color("node_stroke"))
                    .attr("stroke-width", "1")
                    .num("x1", x)
                    .num("y1", MARGIN_TOP + TIMELINE_HEIGHT)
                    .num("x2", x)
                    .num("y2", extent)
                    .attr("stroke-dasharray", "2,2"),
            );
        }
    }

    let mut next_y = (MARGIN_TOP + TIMELINE_HEIGHT) as i64;
    for (si, section) in d.sections.iter().enumerate() {
        let section_y = next_y;
        let section_h = section.tasks.len() as i64 * ROW_HEIGHT;
        doc.add(
            rect(0.0, section_y as f64, width, section_h as f64)
                .attr("fill", color("section_bg"))
                .attr("stroke", "none"),
        );
        doc.add(
            text(&section.name, 10.0, (section_y + section_h / 2) as f64, fonts.normal, &fonts, theme)
                .attr("font-weight", "bold")
                .attr("dominant-baseline", "middle"),
        );
        for (ti, task) in section.tasks.iter().enumerate() {
            let row = section_y + ti as i64 * ROW_HEIGHT;
            doc.add(
                text(&task.description, 10.0, (row + ROW_HEIGHT / 2) as f64, fonts.small, &fonts, theme)
                    .attr("dominant-baseline", "middle"),
            );
            let has = |t: &str| task.tags.iter().any(|x| x == t);
            let role = if has("crit") {
                "error"
            } else if has("done") {
                "success"
            } else if has("active") {
                "info"
            } else {
                "primary"
            };
            let colour = color(role);

            let (start_x, w) = task_timing(task, tl.as_ref());
            let x = MARGIN_LEFT + start_x as f64;
            let y = (row + (ROW_HEIGHT - TASK_BAR_HEIGHT as i64) / 2) as f64;
            if has("milestone") {
                let cy = y + TASK_BAR_HEIGHT / 2.0;
                let size = 12.0;
                let pts = [(x, cy), (x + size, cy - size), (x + size * 2.0, cy), (x + size, cy + size)];
                let points: Vec<String> = pts.iter().map(|(a, b)| format!("{},{}", coord(*a), coord(*b))).collect();
                doc.add(
                    Elem::new("polygon")
                        .attr("fill", colour)
                        .attr("stroke", color("node_stroke"))
                        .attr("stroke-width", "2")
                        .attr("points", points.join(" ")),
                );
            } else {
                doc.add(
                    rect(x, y, w as f64, TASK_BAR_HEIGHT)
                        .attr("fill", colour)
                        .attr("stroke", color("node_stroke"))
                        .attr("stroke-width", "1")
                        .num("rx", 3.0)
                        .num("ry", 3.0),
                );
                // The id label always exists: an unnamed task is called `task_<section>_<index>`.
                if w > 40 {
                    let id = task.id.clone().unwrap_or_else(|| format!("task_{si}_{ti}"));
                    doc.add(
                        text(&id, x + w.div_euclid(2) as f64, y + TASK_BAR_HEIGHT / 2.0, fonts.small, &fonts, theme)
                            .attr("fill", "#ffffff")
                            .attr("text-anchor", "middle")
                            .attr("dominant-baseline", "middle"),
                    );
                }
            }
        }
        next_y += section_h;
    }
    Ok(doc.to_xml())
}

/// Integer coordinates print without a fraction in the milestone `points` string.
fn coord(v: f64) -> String {
    format!("{}", v as i64)
}

/// (snapped start x, width) of a task bar, both integers (`task_timing`).
fn task_timing(task: &Task, tl: Option<&Timeline>) -> (i64, i64) {
    let Some(tl) = tl else { return (0, 20) };
    let ax = task_axis(tl);
    let start_x = match task.calc_start {
        Some(s) if ax.days != 0.0 => ((s - ax.origin) as f64 / DAY as f64 / ax.days) * ax.width,
        _ => 0.0,
    };
    let width = match (task.calc_start, task.render_end.or(task.calc_end)) {
        (Some(s), Some(e)) => {
            let days = (e - s) as f64 / DAY as f64;
            if days <= 0.0 {
                0.0
            } else {
                let w = (days / ax.days) * ax.width;
                if w < 1.0 { 0.0 } else { w }
            }
        }
        _ => 20.0,
    };
    let snapped = start_x.round();
    (snapped as i64, (start_x + width).round() as i64 - snapped as i64)
}
