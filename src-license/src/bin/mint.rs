//! Offline licence minting tool. Vendor-side only — never shipped to users.
//!
//! The signing key is read from `ATMOSPEAK_LICENSE_SIGNING_KEY` (64 hex
//! characters) and must never be committed. `.gitignore` already covers
//! `*.key`; keep the key in a password manager and export it into the
//! environment for the length of a minting session only.
//!
//! Generate a keypair once:
//!
//! ```text
//! cargo run --bin mint -- generate-keypair
//! ```
//!
//! Then mint:
//!
//! ```text
//! ATMOSPEAK_LICENSE_SIGNING_KEY=<hex> \
//!   cargo run --bin mint -- issue --email buyer@example.com --tier pro --updates-until 2027-07-29
//! ```

use std::process::ExitCode;

use atmospeak_license::{License, LicenseTier, hash_email, mint};
use chrono::{NaiveDate, Utc};
use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};

const SIGNING_KEY_ENV: &str = "ATMOSPEAK_LICENSE_SIGNING_KEY";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("generate-keypair") => generate_keypair(),
        Some("issue") => issue(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
Atmospeak licence minting tool

USAGE:
  mint generate-keypair
  mint issue --email <address> [--tier pro|team] [--updates-until YYYY-MM-DD]
             [--seats N] [--key-id N] [--license-id N]

The signing key is read from ATMOSPEAK_LICENSE_SIGNING_KEY (64 hex chars).
--updates-until defaults to one year from today.";

fn generate_keypair() -> Result<(), String> {
    let signing = SigningKey::generate(&mut OsRng);
    let public = signing.verifying_key().to_bytes();

    println!("Private key (set as {SIGNING_KEY_ENV}, never commit this):");
    println!("  {}", to_hex(&signing.to_bytes()));
    println!();
    println!("Public key — paste into TRUSTED_PUBLIC_KEYS in src-license/src/lib.rs:");
    println!("    (1, {public:?}),");

    Ok(())
}

fn issue(args: &[String]) -> Result<(), String> {
    let email = required(args, "--email")?;
    let tier = match optional(args, "--tier")
        .unwrap_or_else(|| "pro".to_string())
        .as_str()
    {
        "pro" => LicenseTier::Pro,
        "team" => LicenseTier::Team,
        other => return Err(format!("unknown tier '{other}' (expected pro or team)")),
    };

    let issued_at = Utc::now().date_naive();
    let updates_until = match optional(args, "--updates-until") {
        Some(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map_err(|_| format!("could not parse '{value}' as YYYY-MM-DD"))?,
        None => issued_at
            .checked_add_months(chrono::Months::new(12))
            .ok_or("could not compute the default update window")?,
    };

    if updates_until < issued_at {
        return Err("--updates-until is before today".to_string());
    }

    let license = License {
        key_id: parse_number(args, "--key-id", 1)?,
        license_id: parse_number(args, "--license-id", random_license_id())?,
        tier,
        email_hash: hash_email(&email),
        issued_at,
        updates_until,
        seats: parse_number(args, "--seats", 1)?,
    };

    let key = mint(&license, &signing_key()?).map_err(|error| error.to_string())?;

    println!("tier:          {}", license.tier.as_str());
    println!("licence id:    {}", license.license_id);
    println!("issued:        {}", license.issued_at);
    println!("updates until: {}", license.updates_until);
    println!("seats:         {}", license.seats);
    println!();
    println!("{key}");

    Ok(())
}

fn signing_key() -> Result<SigningKey, String> {
    let raw = std::env::var(SIGNING_KEY_ENV)
        .map_err(|_| format!("{SIGNING_KEY_ENV} is not set; run generate-keypair first"))?;
    let bytes = from_hex(raw.trim())?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| format!("{SIGNING_KEY_ENV} must be exactly 64 hex characters"))?;
    Ok(SigningKey::from_bytes(&array))
}

fn random_license_id() -> u64 {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    // Keep it positive and readable when printed on an invoice.
    u64::from_be_bytes(bytes) % 1_000_000_000_000
}

fn required(args: &[String], flag: &str) -> Result<String, String> {
    optional(args, flag).ok_or_else(|| format!("{flag} is required"))
}

fn optional(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_number<T: std::str::FromStr>(
    args: &[String],
    flag: &str,
    default: T,
) -> Result<T, String> {
    match optional(args, flag) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("could not parse {flag} value '{value}'")),
        None => Ok(default),
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("signing key must contain an even number of hex characters".to_string());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "signing key contains non-hex characters".to_string())
        })
        .collect()
}
