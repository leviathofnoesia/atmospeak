fn main() {
    embed_release_date();
    tauri_build::build()
}

/// Stamp the build with the date it was produced.
///
/// Licence update windows are compared against this, never against the wall
/// clock: an offline machine with a wrong real-time clock must never lose
/// access to features its owner paid for. See
/// `src-tauri/src/services/license.rs`.
///
/// Release builds set `ATMOSPEAK_RELEASE_DATE` explicitly (see
/// `scripts/package-release.ps1`) so that rebuilding the same release does not
/// move the date. Developer builds fall back to today.
fn embed_release_date() {
    println!("cargo:rerun-if-env-changed=ATMOSPEAK_RELEASE_DATE");

    let date = match std::env::var("ATMOSPEAK_RELEASE_DATE") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                today()
            } else if let Some(valid) = parse_calendar_date(trimmed) {
                valid
            } else {
                panic!(
                    "ATMOSPEAK_RELEASE_DATE must be a real calendar date in YYYY-MM-DD form, got: {trimmed}"
                );
            }
        }
        Err(_) => today(),
    };

    println!("cargo:rustc-env=ATMOSPEAK_RELEASE_DATE={date}");
}

/// Accept only real Gregorian calendar dates (rejects `2026-99-99`).
fn parse_calendar_date(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return None;
    }

    let year: i32 = value[0..4].parse().ok()?;
    let month: u32 = value[5..7].parse().ok()?;
    let day: u32 = value[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    if day > days_in_month {
        return None;
    }

    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn today() -> String {
    // Days since the Unix epoch, converted with Howard Hinnant's
    // civil-from-days algorithm. Written out by hand so build.rs stays
    // dependency-free.
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / 86_400)
        .unwrap_or(0) as i64;

    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}")
}
