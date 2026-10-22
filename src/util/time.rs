//! Calendar formatting without a date crate: unix seconds to broken-down
//! time and the handful of textual formats servers and logs use.

/// Broken-down UTC time (plus a fixed offset applied by the caller).
#[derive(Clone, Copy, Debug)]
pub struct Tm {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    /// 0 = Sunday.
    pub weekday: u32,
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTH_NAMES: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

impl Tm {
    /// The current UTC time.
    pub fn now() -> Self {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Tm::at(secs, 0)
    }

    /// From unix seconds; `offset_min` is added before breaking down so the
    /// result shows local wall-clock time for that offset.
    pub fn at(unix: i64, offset_min: i32) -> Self {
        let t = unix + offset_min as i64 * 60;
        let days = t.div_euclid(86_400);
        let rem = t.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Tm {
            year,
            month,
            day,
            hour: (rem / 3600) as u32,
            min: ((rem % 3600) / 60) as u32,
            sec: (rem % 60) as u32,
            weekday: ((days + 4).rem_euclid(7)) as u32,
        }
    }

    pub fn mon(&self) -> &'static str {
        MONTHS[(self.month - 1) as usize]
    }

    pub fn wday(&self) -> &'static str {
        WEEKDAYS[self.weekday as usize]
    }

    /// `2026-10-21`
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `13:05:09`
    pub fn time(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.min, self.sec)
    }

    /// `Wed, 21 Oct 2026 13:05:09 GMT` (only meaningful with offset 0).
    pub fn http(&self) -> String {
        format!("{}, {:02} {} {:04} {} GMT", self.wday(), self.day, self.mon(), self.year, self.time())
    }

    /// `21/Oct/2026:13:05:09`
    pub fn clf(&self) -> String {
        format!("{:02}/{}/{:04}:{}", self.day, self.mon(), self.year, self.time())
    }
}

/// `+0200` / `-0500`
pub fn offset_compact(offset_min: i32) -> String {
    let sign = if offset_min < 0 { '-' } else { '+' };
    let m = offset_min.abs();
    format!("{sign}{:02}{:02}", m / 60, m % 60)
}

/// `+02:00`
pub fn offset_colon(offset_min: i32) -> String {
    let sign = if offset_min < 0 { '-' } else { '+' };
    let m = offset_min.abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// `strftime`-style formatting for the specifiers templates use:
/// `%Y %y %m %d %e %H %I %M %S %p %b %B %a %A %j %z %:z %f %3f %s %%`.
/// `offset_min` shifts the wall clock and feeds `%z`; `micros` feeds `%f`.
pub fn strftime(unix: i64, micros: u32, offset_min: i32, fmt: &str) -> String {
    let t = Tm::at(unix, offset_min);
    let mut out = String::with_capacity(fmt.len() + 16);
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // `%-d`, `%-m`, `%-I`, ...: the same field without zero padding.
        let unpadded = chars.peek() == Some(&'-');
        if unpadded {
            chars.next();
        }
        let start = out.len();
        match chars.next() {
            Some('Y') => out += &format!("{:04}", t.year),
            Some('y') => out += &format!("{:02}", t.year.rem_euclid(100)),
            Some('m') => out += &format!("{:02}", t.month),
            Some('d') => out += &format!("{:02}", t.day),
            Some('e') => out += &format!("{:>2}", t.day),
            Some('H') => out += &format!("{:02}", t.hour),
            Some('I') => out += &format!("{:02}", (t.hour + 11) % 12 + 1),
            Some('M') => out += &format!("{:02}", t.min),
            Some('S') => out += &format!("{:02}", t.sec),
            Some('p') => out += if t.hour < 12 { "AM" } else { "PM" },
            Some('b') => out += t.mon(),
            Some('a') => out += t.wday(),
            Some('A') => out += WEEKDAY_NAMES[t.weekday as usize],
            Some('B') => out += MONTH_NAMES[(t.month - 1) as usize],
            Some('j') => out += &format!("{:03}", day_of_year(&t)),
            Some('z') => out += &offset_compact(offset_min),
            Some(':') if chars.peek() == Some(&'z') => {
                chars.next();
                out += &offset_colon(offset_min);
            }
            Some('f') => out += &format!("{micros:06}"),
            Some('3') if chars.peek() == Some(&'f') => {
                chars.next();
                out += &format!("{:03}", micros / 1000);
            }
            Some('s') => out += &unix.to_string(),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
        if unpadded {
            let field = out.split_off(start);
            let trimmed = field.trim_start_matches(['0', ' ']);
            out += if trimmed.is_empty() { "0" } else { trimmed };
        }
    }
    out
}

fn day_of_year(t: &Tm) -> u32 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (t.year % 4 == 0 && t.year % 100 != 0) || t.year % 400 == 0;
    CUM[(t.month - 1) as usize] + t.day + u32::from(leap && t.month > 2)
}

/// Unix time of midnight UTC on a `YYYY-MM-DD` date.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut it = text.split('-').map(|p| p.parse::<i64>());
    let (y, m, d) = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || it.next().is_some() {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400)
}

/// Howard Hinnant's civil-to-days algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar() {
        // 2026-10-21 13:05:09 UTC was a Wednesday.
        let t = Tm::at(1_792_587_909, 0);
        assert_eq!(t.date(), "2026-10-21");
        assert_eq!(t.time(), "13:05:09");
        assert_eq!(t.wday(), "Wed");
        assert_eq!(Tm::at(0, 0).date(), "1970-01-01");
        assert_eq!(Tm::at(951_782_400, 0).date(), "2000-02-29");
    }

    #[test]
    fn formats() {
        let t = Tm::at(1_792_587_909, 0);
        assert_eq!(t.http(), "Wed, 21 Oct 2026 13:05:09 GMT");
        assert_eq!(t.clf(), "21/Oct/2026:13:05:09");
        assert_eq!(Tm::at(1_792_587_909, 120).time(), "15:05:09");
        assert_eq!(offset_compact(-300), "-0500");
        assert_eq!(offset_colon(330), "+05:30");
    }

    #[test]
    fn parse_date_round_trips() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        let t = parse_date("2026-10-21").unwrap();
        assert_eq!(Tm::at(t, 0).date(), "2026-10-21");
        assert_eq!(parse_date("2000-02-29").map(|t| Tm::at(t, 0).date()), Some("2000-02-29".into()));
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("nope"), None);
    }

    #[test]
    fn strftime_specifiers() {
        let t = 1_792_587_909;
        assert_eq!(strftime(t, 0, 0, "%d/%b/%Y:%H:%M:%S %z"), "21/Oct/2026:13:05:09 +0000");
        assert_eq!(strftime(t, 123_456, 60, "%Y-%m-%dT%H:%M:%S.%f%:z"), "2026-10-21T14:05:09.123456+01:00");
        assert_eq!(strftime(t, 123_456, 0, "%H:%M:%S,%3f"), "13:05:09,123");
        assert_eq!(strftime(t, 0, 0, "%a %e %I%p %j %s %% %q"), "Wed 21 01PM 294 1792587909 % %q");
        assert_eq!(strftime(t, 0, 0, "%A %B"), "Wednesday October");
        assert_eq!(strftime(t, 0, 0, "%-m/%-d/%Y %-I:%M %p"), "10/21/2026 1:05 PM");
    }
}
