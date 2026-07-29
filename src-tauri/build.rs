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

    let date = std::env::var("ATMOSPEAK_RELEASE_DATE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| is_iso_date(value))
        .unwrap_or_else(today);

    println!("cargo:rustc-env=ATMOSPEAK_RELEASE_DATE={date}");
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
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
