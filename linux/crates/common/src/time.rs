use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn local(seconds: i64) -> Civil {
    let time = seconds as libc::time_t;
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&time, &mut parts) }.is_null() {
        return utc(seconds);
    }
    Civil {
        year: parts.tm_year as i64 + 1900,
        month: parts.tm_mon as u32 + 1,
        day: parts.tm_mday as u32,
        hour: parts.tm_hour as u32,
        minute: parts.tm_min as u32,
        second: parts.tm_sec as u32,
    }
}

pub fn utc(seconds: i64) -> Civil {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400) as u32;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Civil { year, month, day, hour: rest / 3600, minute: rest / 60 % 60, second: rest % 60 }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub fn file_stamp(seconds: i64) -> String {
    let t = local(seconds);
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

pub fn clock(seconds: i64) -> String {
    let t = local(seconds);
    format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
}

const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];

pub fn short_date(seconds: i64) -> String {
    let t = local(seconds);
    format!("{} {} {}, {:02}:{:02}", t.day, MONTHS[(t.month as usize).clamp(1, 12) - 1], t.year, t.hour, t.minute)
}

pub fn iso8601(seconds: i64) -> String {
    let t = utc(seconds);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

pub fn parse_iso8601(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, clock) = text.split_once('T')?;
    let mut date = date.splitn(3, '-').map(str::parse::<i64>);
    let mut clock = clock.splitn(3, ':').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let (hour, minute, second) = (clock.next()?.ok()?, clock.next()?.ok()?, clock.next()?.ok()?);
    Some(days_from_civil(year, month as u32, day as u32) * 86_400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_round_trips_across_leap_days() {
        for seconds in [0, 951_782_400, 1_709_208_000, 1_793_000_000] {
            assert_eq!(parse_iso8601(&iso8601(seconds)), Some(seconds));
        }
        assert_eq!(iso8601(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn short_dates_name_the_month_in_spanish() {
        let date = short_date(1_709_208_000);
        let t = local(1_709_208_000);
        assert_eq!(date, format!("{} feb 2024, {:02}:{:02}", t.day, t.hour, t.minute));
    }
}
