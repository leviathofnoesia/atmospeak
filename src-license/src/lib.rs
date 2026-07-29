//! Offline-verifiable licence keys for Atmospeak.
//!
//! A licence key is a compact payload signed with an Ed25519 private key that
//! never leaves the vendor. The application embeds only public keys, so
//! verification is a pure local computation: **no activation server, no
//! phone-home, no network access of any kind.** That guarantee is a product
//! feature, not an implementation detail — see `docs/STRATEGY.md`.
//!
//! This crate deliberately depends on nothing platform-specific. Keyring
//! storage and application wiring live in `src-tauri/src/services/license.rs`;
//! the minting tool is `src/bin/mint.rs` in this crate. Sharing one
//! implementation between minting and verification is the point: a format
//! defined twice is a format that will drift.
//!
//! # Threat model
//!
//! Offline verification can be defeated by patching the binary, and Atmospeak's
//! core is MIT-licensed source anyway. That is an accepted trade. Someone
//! willing to patch a binary was never going to buy a licence, and the offline
//! guarantee is worth more in trust than any anti-tamper measure is in
//! prevented losses. Do not add measures here that require a network call.

use chrono::NaiveDate;
use data_encoding::BASE32_NOPAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Human-facing prefix so a support inbox can recognise a key on sight.
pub const KEY_PREFIX: &str = "ATMO";

/// Characters per hyphen-separated group in the rendered key.
const GROUP_LEN: usize = 5;

/// Ed25519 signatures are fixed width; anything shorter is malformed.
const SIGNATURE_LEN: usize = 64;

/// Public keys trusted by this build, keyed by `key_id`.
///
/// Carrying a set rather than a single key means a compromised signing key can
/// be rotated by shipping a build that trusts a new `key_id`, without
/// invalidating licences already sold under the old one.
///
/// The placeholder below is all-zero and rejects everything, which is the
/// correct default: an unconfigured build sells nothing rather than trusting
/// anything. Populate it from `cargo run --bin mint -- generate-keypair`.
pub const TRUSTED_PUBLIC_KEYS: &[(u8, [u8; 32])] = &[(1, [0u8; 32])];

/// Licence tiers, ordered so that `Team` satisfies any `Pro` requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LicenseTier {
    Free,
    Pro,
    Team,
}

impl LicenseTier {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Free),
            1 => Some(Self::Pro),
            2 => Some(Self::Team),
            _ => None,
        }
    }

    fn to_wire(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Pro => 1,
            Self::Team => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Pro => "pro",
            Self::Team => "team",
        }
    }
}

/// Capabilities that a licence can unlock.
///
/// Every variant here is functionality that did not exist in Atmospeak 0.5.3.
/// Nothing already shipped is represented in this enum, and nothing already
/// shipped may be added to it later — see `docs/STRATEGY.md` §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// Attested airplane mode, network ledger, audit log export.
    CompliancePack,
    /// Dictation that dispatches an allowlisted command instead of pasting.
    VoiceMacros,
    /// Model Context Protocol server exposing dictation to coding agents.
    McpServer,
    /// Editor symbol awareness and per-application cleanup profiles.
    IdeAwareness,
    /// Encrypted cross-device sync of dictionary, snippets, and settings.
    Sync,
    /// Shared team vocabulary and seat provisioning.
    TeamSharedVocabulary,
}

impl Feature {
    const fn required_tier(self) -> LicenseTier {
        match self {
            Self::TeamSharedVocabulary => LicenseTier::Team,
            _ => LicenseTier::Pro,
        }
    }
}

/// A verified licence. Fields are private so only [`verify`] / [`verify_with`]
/// and [`License::issue`] (minting) can construct one — callers outside this
/// crate cannot forge a `License` by struct literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct License {
    key_id: u8,
    license_id: u64,
    tier: LicenseTier,
    /// First 8 bytes of the SHA-256 of the purchaser's normalised address. Ties
    /// a key to a buyer for support purposes without ever storing the address.
    email_hash: [u8; 8],
    issued_at: NaiveDate,
    /// Last build release date this licence entitles the holder to run.
    updates_until: NaiveDate,
    seats: u8,
}

impl License {
    /// Construct a licence for minting. The application path only obtains a
    /// `License` through [`verify`].
    pub fn issue(
        key_id: u8,
        license_id: u64,
        tier: LicenseTier,
        email_hash: [u8; 8],
        issued_at: NaiveDate,
        updates_until: NaiveDate,
        seats: u8,
    ) -> Self {
        Self {
            key_id,
            license_id,
            tier,
            email_hash,
            issued_at,
            updates_until,
            seats,
        }
    }

    pub fn key_id(&self) -> u8 {
        self.key_id
    }

    pub fn license_id(&self) -> u64 {
        self.license_id
    }

    pub fn tier(&self) -> LicenseTier {
        self.tier
    }

    pub fn email_hash(&self) -> [u8; 8] {
        self.email_hash
    }

    pub fn issued_at(&self) -> NaiveDate {
        self.issued_at
    }

    pub fn updates_until(&self) -> NaiveDate {
        self.updates_until
    }

    pub fn seats(&self) -> u8 {
        self.seats
    }
}

/// What the running build is allowed to do. Always obtainable — the free
/// variant is a valid answer, never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    tier: LicenseTier,
    in_update_window: bool,
}

impl Entitlements {
    /// The answer for an absent, malformed, forged, or unreadable licence.
    /// Every failure path in this crate and its callers resolves here.
    pub const fn free() -> Self {
        Self {
            tier: LicenseTier::Free,
            in_update_window: false,
        }
    }

    /// Resolve entitlements for a licence against the running build.
    ///
    /// `build_released_on` is the release date compiled into the binary, **not**
    /// the wall clock. An offline machine with a wrong real-time clock must
    /// never lose access to features it paid for, and a user who never updates
    /// must never be nagged. The comparison is therefore between two facts that
    /// are both fixed at build and purchase time.
    pub fn for_license(license: &License, build_released_on: NaiveDate) -> Self {
        Self {
            tier: license.tier(),
            in_update_window: build_released_on <= license.updates_until(),
        }
    }

    /// The single choke point for every entitlement decision in the app.
    pub fn allows(&self, feature: Feature) -> bool {
        self.in_update_window && self.tier >= feature.required_tier()
    }

    pub fn tier(&self) -> LicenseTier {
        self.tier
    }

    /// False when a paid licence is present but the running build was released
    /// after the update window closed. Callers surface this as "renew for
    /// updates", never as "your software stopped working".
    pub fn in_update_window(&self) -> bool {
        self.in_update_window
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseError {
    Empty,
    BadPrefix,
    BadEncoding,
    TooShort,
    MalformedPayload,
    UnknownTier(u8),
    /// The key names a signing key this build does not trust. Either a forgery
    /// or a licence issued under a key that has since been rotated out.
    UntrustedKeyId(u8),
    BadSignature,
    InvalidDate,
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Empty => "licence key is empty",
            Self::BadPrefix => "licence key does not start with the expected prefix",
            Self::BadEncoding => "licence key contains characters that are not valid",
            Self::TooShort => "licence key is truncated",
            Self::MalformedPayload => "licence key payload could not be read",
            Self::UnknownTier(_) => "licence key names a tier this version does not understand",
            Self::UntrustedKeyId(_) => "licence key was not issued by a recognised signing key",
            Self::BadSignature => "licence key signature is not valid",
            Self::InvalidDate => "licence key contains a date that is out of range",
        };
        f.write_str(message)
    }
}

impl std::error::Error for LicenseError {}

/// The signed wire payload. Field order is part of the format; append new
/// fields only at the end, and only alongside a `key_id` bump.
#[derive(Serialize, Deserialize)]
struct Payload {
    key_id: u8,
    license_id: u64,
    tier: u8,
    email_hash: [u8; 8],
    issued_at_days: u32,
    updates_until_days: u32,
    seats: u8,
}

/// Hash a purchaser's address for embedding. Normalised so that trivial
/// variations of the same address produce the same hash.
pub fn hash_email(email: &str) -> [u8; 8] {
    let digest = Sha256::digest(email.trim().to_ascii_lowercase().as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

fn days_from_date(date: NaiveDate) -> Option<u32> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    u32::try_from((date - epoch).num_days()).ok()
}

fn date_from_days(days: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(1970, 1, 1)?.checked_add_days(chrono::Days::new(u64::from(days)))
}

/// Render a signed licence key. Used by the minting tool; the application only
/// ever verifies.
pub fn mint(license: &License, signing_key: &SigningKey) -> Result<String, LicenseError> {
    let payload = Payload {
        key_id: license.key_id(),
        license_id: license.license_id(),
        tier: license.tier().to_wire(),
        email_hash: license.email_hash(),
        issued_at_days: days_from_date(license.issued_at()).ok_or(LicenseError::InvalidDate)?,
        updates_until_days: days_from_date(license.updates_until())
            .ok_or(LicenseError::InvalidDate)?,
        seats: license.seats(),
    };

    let body = postcard::to_allocvec(&payload).map_err(|_| LicenseError::MalformedPayload)?;
    let signature = signing_key.sign(&body);

    let mut raw = body;
    raw.extend_from_slice(&signature.to_bytes());

    Ok(format_key(&BASE32_NOPAD.encode(&raw)))
}

fn format_key(encoded: &str) -> String {
    let mut out = String::with_capacity(encoded.len() + encoded.len() / GROUP_LEN + 8);
    out.push_str(KEY_PREFIX);
    for (index, character) in encoded.chars().enumerate() {
        if index % GROUP_LEN == 0 {
            out.push('-');
        }
        out.push(character);
    }
    out
}

/// Strip formatting so that keys survive copy/paste through chat clients,
/// spreadsheets, and hand transcription.
fn normalise(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Verify a licence key against the public keys this build trusts.
pub fn verify(key: &str) -> Result<License, LicenseError> {
    verify_with(key, TRUSTED_PUBLIC_KEYS)
}

/// Verify against an explicit key set. Exposed so tests can exercise forgery
/// and rotation without touching the shipped constant.
pub fn verify_with(key: &str, trusted: &[(u8, [u8; 32])]) -> Result<License, LicenseError> {
    let normalised = normalise(key);
    if normalised.is_empty() {
        return Err(LicenseError::Empty);
    }

    let body = normalised
        .strip_prefix(KEY_PREFIX)
        .ok_or(LicenseError::BadPrefix)?;

    let raw = BASE32_NOPAD
        .decode(body.as_bytes())
        .map_err(|_| LicenseError::BadEncoding)?;

    if raw.len() <= SIGNATURE_LEN {
        return Err(LicenseError::TooShort);
    }

    let (payload_bytes, signature_bytes) = raw.split_at(raw.len() - SIGNATURE_LEN);

    // Read the payload before checking the signature so we know which public
    // key to check against. Nothing is trusted until the signature verifies.
    let payload: Payload =
        postcard::from_bytes(payload_bytes).map_err(|_| LicenseError::MalformedPayload)?;

    let public_key_bytes = trusted
        .iter()
        .find(|(id, _)| *id == payload.key_id)
        .map(|(_, bytes)| bytes)
        .ok_or(LicenseError::UntrustedKeyId(payload.key_id))?;

    let verifying_key =
        VerifyingKey::from_bytes(public_key_bytes).map_err(|_| LicenseError::BadSignature)?;

    let signature_array: [u8; SIGNATURE_LEN] = signature_bytes
        .try_into()
        .map_err(|_| LicenseError::TooShort)?;

    verifying_key
        .verify_strict(payload_bytes, &Signature::from_bytes(&signature_array))
        .map_err(|_| LicenseError::BadSignature)?;

    let tier =
        LicenseTier::from_wire(payload.tier).ok_or(LicenseError::UnknownTier(payload.tier))?;

    Ok(License {
        key_id: payload.key_id,
        license_id: payload.license_id,
        tier,
        email_hash: payload.email_hash,
        issued_at: date_from_days(payload.issued_at_days).ok_or(LicenseError::InvalidDate)?,
        updates_until: date_from_days(payload.updates_until_days)
            .ok_or(LicenseError::InvalidDate)?,
        seats: payload.seats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid test date")
    }

    fn keypair() -> (SigningKey, [u8; 32]) {
        let signing = SigningKey::generate(&mut OsRng);
        let public = signing.verifying_key().to_bytes();
        (signing, public)
    }

    fn sample(tier: LicenseTier) -> License {
        License::issue(
            1,
            4_242_424_242,
            tier,
            hash_email("Buyer@Example.COM "),
            date(2026, 7, 29),
            date(2027, 7, 29),
            1,
        )
    }

    fn sample_with_key_id(tier: LicenseTier, key_id: u8) -> License {
        License::issue(
            key_id,
            4_242_424_242,
            tier,
            hash_email("Buyer@Example.COM "),
            date(2026, 7, 29),
            date(2027, 7, 29),
            1,
        )
    }

    fn sample_with_dates(
        tier: LicenseTier,
        issued_at: NaiveDate,
        updates_until: NaiveDate,
    ) -> License {
        License::issue(
            1,
            4_242_424_242,
            tier,
            hash_email("Buyer@Example.COM "),
            issued_at,
            updates_until,
            1,
        )
    }

    #[test]
    fn round_trips_a_valid_key() {
        let (signing, public) = keypair();
        let license = sample(LicenseTier::Pro);
        let key = mint(&license, &signing).expect("mint succeeds");

        let verified = verify_with(&key, &[(1, public)]).expect("verify succeeds");
        assert_eq!(verified, license);
    }

    #[test]
    fn rendered_key_is_prefixed_and_grouped() {
        let (signing, _) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");

        assert!(key.starts_with("ATMO-"));
        assert!(key.contains('-'));
    }

    #[test]
    fn email_hash_normalises_case_and_whitespace() {
        assert_eq!(
            hash_email("  Buyer@Example.com "),
            hash_email("buyer@example.com")
        );
        assert_ne!(hash_email("a@example.com"), hash_email("b@example.com"));
    }

    #[test]
    fn accepts_keys_with_mangled_formatting() {
        let (signing, public) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");

        let mangled = format!("  {}  ", key.replace('-', " ").to_ascii_lowercase());
        assert!(verify_with(&mangled, &[(1, public)]).is_ok());
    }

    #[test]
    fn rejects_a_key_signed_by_a_different_keypair() {
        let (signing, _) = keypair();
        let (_, other_public) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");

        assert_eq!(
            verify_with(&key, &[(1, other_public)]),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let (signing, public) = keypair();
        // Mint Pro, then attempt to promote the licence to Team by editing the
        // encoded body. The signature covers the payload, so this must fail.
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");

        let normalised = normalise(&key);
        let body = normalised.strip_prefix(KEY_PREFIX).expect("prefix present");
        let raw = BASE32_NOPAD.decode(body.as_bytes()).expect("decodes");
        let (payload_bytes, signature_bytes) = raw.split_at(raw.len() - SIGNATURE_LEN);

        // Rewrite the payload through the real deserialiser so the forgery is
        // structurally valid, then reattach the genuine signature.
        let mut payload: Payload = postcard::from_bytes(payload_bytes).expect("payload decodes");
        assert_eq!(payload.tier, LicenseTier::Pro.to_wire());
        payload.tier = LicenseTier::Team.to_wire();

        let mut forged_raw = postcard::to_allocvec(&payload).expect("payload re-encodes");
        forged_raw.extend_from_slice(signature_bytes);

        let forged = format_key(&BASE32_NOPAD.encode(&forged_raw));
        assert_eq!(
            verify_with(&forged, &[(1, public)]),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn rejects_an_untrusted_key_id() {
        let (signing, public) = keypair();
        let license = sample_with_key_id(LicenseTier::Pro, 9);
        let key = mint(&license, &signing).expect("mint succeeds");

        assert_eq!(
            verify_with(&key, &[(1, public)]),
            Err(LicenseError::UntrustedKeyId(9))
        );
    }

    #[test]
    fn rotation_keeps_older_licences_valid() {
        let (old_signing, old_public) = keypair();
        let (new_signing, new_public) = keypair();

        let old_license = sample_with_key_id(LicenseTier::Pro, 1);
        let new_license = sample_with_key_id(LicenseTier::Pro, 2);

        let trusted = [(1, old_public), (2, new_public)];
        assert!(verify_with(&mint(&old_license, &old_signing).unwrap(), &trusted).is_ok());
        assert!(verify_with(&mint(&new_license, &new_signing).unwrap(), &trusted).is_ok());
    }

    #[test]
    fn rejects_empty_and_prefix_only_input() {
        let (_, public) = keypair();
        assert_eq!(verify_with("", &[(1, public)]), Err(LicenseError::Empty));
        assert_eq!(verify_with("   ", &[(1, public)]), Err(LicenseError::Empty));
        assert_eq!(
            verify_with("ATMO", &[(1, public)]),
            Err(LicenseError::TooShort)
        );
    }

    #[test]
    fn rejects_a_key_without_the_prefix() {
        let (signing, public) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");
        let without_prefix = key.replacen(KEY_PREFIX, "XXXX", 1);

        assert_eq!(
            verify_with(&without_prefix, &[(1, public)]),
            Err(LicenseError::BadPrefix)
        );
    }

    #[test]
    fn rejects_a_truncated_key() {
        let (signing, public) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");
        let truncated: String = key.chars().take(key.len() / 2).collect();

        assert!(matches!(
            verify_with(&truncated, &[(1, public)]),
            Err(LicenseError::BadSignature | LicenseError::TooShort | LicenseError::BadEncoding)
        ));
    }

    #[test]
    fn rejects_a_key_with_appended_bytes() {
        let (signing, public) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");

        assert_eq!(
            verify_with(&format!("{key}AAAAAAAA"), &[(1, public)]),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn rejects_characters_outside_the_alphabet() {
        let (_, public) = keypair();
        // '0', '1', and '8' are not in the RFC 4648 base32 alphabet.
        assert_eq!(
            verify_with("ATMO-01801-80180", &[(1, public)]),
            Err(LicenseError::BadEncoding)
        );
    }

    #[test]
    fn the_shipped_placeholder_key_trusts_nothing() {
        // A build that has not had a real public key configured must sell
        // nothing rather than trust anything.
        let (signing, _) = keypair();
        let key = mint(&sample(LicenseTier::Pro), &signing).expect("mint succeeds");
        assert!(verify(&key).is_err());
    }

    #[test]
    fn free_entitlements_allow_nothing() {
        let free = Entitlements::free();
        for feature in [
            Feature::CompliancePack,
            Feature::VoiceMacros,
            Feature::McpServer,
            Feature::IdeAwareness,
            Feature::Sync,
            Feature::TeamSharedVocabulary,
        ] {
            assert!(
                !free.allows(feature),
                "free tier must not allow {feature:?}"
            );
        }
        assert_eq!(free.tier(), LicenseTier::Free);
    }

    #[test]
    fn pro_allows_pro_features_but_not_team_features() {
        let license = sample(LicenseTier::Pro);
        let entitlements = Entitlements::for_license(&license, date(2026, 8, 1));

        assert!(entitlements.allows(Feature::CompliancePack));
        assert!(entitlements.allows(Feature::McpServer));
        assert!(!entitlements.allows(Feature::TeamSharedVocabulary));
    }

    #[test]
    fn team_satisfies_pro_requirements() {
        let license = sample(LicenseTier::Team);
        let entitlements = Entitlements::for_license(&license, date(2026, 8, 1));

        assert!(entitlements.allows(Feature::CompliancePack));
        assert!(entitlements.allows(Feature::TeamSharedVocabulary));
    }

    #[test]
    fn a_build_released_after_the_window_falls_back_to_free() {
        let license = sample(LicenseTier::Pro);
        let entitlements = Entitlements::for_license(&license, date(2027, 7, 30));

        assert!(!entitlements.in_update_window());
        assert!(!entitlements.allows(Feature::CompliancePack));
        // The licence itself remains valid and readable; only this build is out
        // of window. Reinstalling an older build restores the features.
        assert_eq!(entitlements.tier(), LicenseTier::Pro);
    }

    #[test]
    fn a_build_released_on_the_last_day_is_still_in_window() {
        let license = sample(LicenseTier::Pro);
        let entitlements = Entitlements::for_license(&license, date(2027, 7, 29));

        assert!(entitlements.in_update_window());
        assert!(entitlements.allows(Feature::Sync));
    }

    #[test]
    fn dates_survive_the_round_trip() {
        let (signing, public) = keypair();
        let license = sample_with_dates(LicenseTier::Pro, date(2028, 2, 29), date(2099, 12, 31));

        let key = mint(&license, &signing).expect("mint succeeds");
        let verified = verify_with(&key, &[(1, public)]).expect("verify succeeds");

        assert_eq!(verified.issued_at(), date(2028, 2, 29));
        assert_eq!(verified.updates_until(), date(2099, 12, 31));
    }
}
