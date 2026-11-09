//! The board's secrets, in one place: random codes, passwords, authenticator
//! codes and the seeds behind them. It follows the rules of United One's
//! backend (ADR-0013 and ADR-0024 of arcanii/United_ONE-BackendSystem):
//!
//! - Emailed codes are 6 digits from the OS's randomness, drawn without the
//!   bias `% 10` gives, kept only as a keyed hash, used once, for 10
//!   minutes, 5 tries.
//! - Authenticator codes are RFC 6238's: 6 digits, 30-second steps, a step
//!   either side and no more, a step once used never taken again, and
//!   compared in constant time.
//! - A seed is kept sealed (XChaCha20-Poly1305) under a key the database
//!   doesn't hold, bound to its member, so a dump of the database gives
//!   neither the seeds nor a way to try codes and recovery codes offline.
//!
//! Unlike United One's break-glass, the codes are HMAC-SHA1, RFC 6238's
//! default, because members bring their own authenticator apps and Google
//! Authenticator ignores any other (it shows SHA-1 codes for a SHA-256 seed,
//! which never verify). HMAC-SHA1 has no known weakness as a code's MAC.

use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// `N` bytes from the OS's randomness.
pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS's randomness is unavailable");
    b
}

/// `n` digits, each equally likely: a byte of 250 or more is drawn again,
/// or 0 to 5 would come up more often than 6 to 9.
pub fn digits(n: usize) -> String {
    let mut out = String::with_capacity(n);
    while out.len() < n {
        for b in random::<16>() {
            if b < 250 && out.len() < n {
                out.push(char::from(b'0' + b % 10));
            }
        }
    }
    out
}

/// Whether `a` and `b` are the same, taking as long either way.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// The board's own key, from which the sealing key and the hashing key are
/// drawn, each for its one use.
#[derive(Clone)]
pub struct Keys {
    seal: [u8; 32],
    hash: [u8; 32],
}

impl Keys {
    /// From the board's secret: 32 bytes, as 64 hex digits.
    pub fn from_hex(hex: &str) -> Result<Keys, String> {
        let bytes = data_encoding::HEXLOWER_PERMISSIVE.decode(hex.trim().as_bytes()).map_err(|_| "ANIE_BBS_SECRET_KEY must be 64 hex digits".to_string())?;
        let master: [u8; 32] = bytes.try_into().map_err(|_| "ANIE_BBS_SECRET_KEY must be 64 hex digits (32 bytes)".to_string())?;
        let derive = |what: &[u8]| -> [u8; 32] {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&master).expect("any key length");
            mac.update(b"anie-bbs ");
            mac.update(what);
            mac.finalize().into_bytes().into()
        };
        Ok(Keys { seal: derive(b"seal"), hash: derive(b"hash") })
    }

    /// A keyed hash of a code or recovery code, as hex, for keeping: without
    /// the key, a stolen hash can't be tried against guesses.
    pub fn hash(&self, purpose: &str, code: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.hash).expect("any key length");
        mac.update(purpose.as_bytes());
        mac.update(b"\0");
        mac.update(code.as_bytes());
        data_encoding::HEXLOWER.encode(&mac.finalize().into_bytes())
    }

    /// `plain` sealed for member `id`: a nonce, then the sealed bytes.
    pub fn seal(&self, id: i64, plain: &[u8]) -> Vec<u8> {
        let cipher = XChaCha20Poly1305::new((&self.seal).into());
        let nonce = random::<24>();
        let aad = id.to_be_bytes();
        let sealed = cipher.encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: &aad }).expect("sealing can't fail");
        [nonce.as_slice(), &sealed].concat()
    }

    /// What `seal` sealed for member `id`; `None` if it was sealed for
    /// another, under another key, or touched.
    pub fn open(&self, id: i64, sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < 24 {
            return None;
        }
        let (nonce, body) = sealed.split_at(24);
        let cipher = XChaCha20Poly1305::new((&self.seal).into());
        cipher.decrypt(XNonce::from_slice(nonce), Payload { msg: body, aad: &id.to_be_bytes() }).ok()
    }
}

/// A password hashed for keeping (Argon2id, its salt and settings with it).
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::encode_b64(&random::<16>()).expect("16 bytes is a salt");
    Argon2::default().hash_password(password.as_bytes(), &salt).expect("hashing can't fail").to_string()
}

/// Whether `password` is the one `hash` was made from. With no hash it
/// takes as long, so a handle that isn't there isn't told apart by time.
pub fn check_password(password: &str, hash: Option<&str>) -> bool {
    static NOBODY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let nobody = NOBODY.get_or_init(|| hash_password("nobody's password"));
    let real = hash.is_some();
    let Ok(parsed) = PasswordHash::new(hash.unwrap_or(nobody)) else { return false };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok() && real
}

/// A recovery code: ten letters and digits that can't be misread for one
/// another, in two fives.
pub fn recovery_code() -> String {
    const LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    // A byte past the last whole run of 31 is drawn again, as `digits` does.
    let limit = 256 - 256 % LETTERS.len();
    let mut out = String::with_capacity(11);
    while out.len() < 11 {
        for b in random::<16>() {
            if out.len() == 5 {
                out.push('-');
            }
            if (b as usize) < limit && out.len() < 11 {
                out.push(LETTERS[b as usize % LETTERS.len()] as char);
            }
        }
    }
    out
}

/// A recovery code as typed, made plain: no dash, no spaces, lowercase.
pub fn plain_recovery_code(typed: &str) -> String {
    typed.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// The 30-second step `at` falls in.
pub fn step(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) / 30
}

/// The step now.
pub fn step_now() -> u64 {
    step(SystemTime::now())
}

/// A new authenticator seed: 20 bytes, as RFC 4226 asks for HMAC-SHA1.
pub fn new_seed() -> [u8; 20] {
    random()
}

/// The code for `step` (RFC 6238's TOTP, from RFC 4226's HOTP).
pub fn code(seed: &[u8], step: u64, digits: u32) -> String {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(seed).expect("any key length");
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let at = (h[19] & 0x0f) as usize;
    let n = u32::from_be_bytes([h[at] & 0x7f, h[at + 1], h[at + 2], h[at + 3]]) % 10u32.pow(digits);
    format!("{n:0width$}", width = digits as usize)
}

/// The step `typed` is the code for, a step either side of `now` and no
/// further, and only past `last` (the last step taken), so a code once used
/// can't be used again; `None` where it's none of them. Every step is tried,
/// so it takes as long whichever it was.
pub fn check_code(seed: &[u8], typed: &str, now: u64, last: Option<u64>) -> Option<u64> {
    let typed = typed.trim();
    if typed.len() != 6 || !typed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut found = None;
    for s in [now.saturating_sub(1), now, now + 1] {
        if same(code(seed, s, 6).as_bytes(), typed.as_bytes()) && last.is_none_or(|l| s > l) && found.is_none() {
            found = Some(s);
        }
    }
    found
}

/// A seed as base32, for typing into an app by hand.
pub fn seed_text(seed: &[u8]) -> String {
    data_encoding::BASE32_NOPAD.encode(seed)
}

/// The `otpauth://` address an app reads from the QR code.
pub fn seed_uri(seed: &[u8], issuer: &str, account: &str) -> String {
    let esc = |s: &str| -> String {
        s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    };
    format!("otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30", esc(issuer), esc(account), seed_text(seed), esc(issuer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn codes_are_rfc_6238s_for_its_sha1_test_seed() {
        // RFC 6238 appendix B: the seed "12345678901234567890", 8 digits.
        let seed = b"12345678901234567890";
        for (secs, want) in [(59u64, "94287082"), (1111111109, "07081804"), (1111111111, "14050471"), (1234567890, "89005924"), (2000000000, "69279037"), (20000000000, "65353130")] {
            assert_eq!(code(seed, step(UNIX_EPOCH + Duration::from_secs(secs)), 8), want, "at {secs}");
        }
        assert_eq!(code(seed, 1, 6), "287082", "six digits are the last six");
    }

    #[test]
    fn a_code_is_taken_a_step_either_side_once_and_never_again() {
        let seed = new_seed();
        let now = 1_000_000;
        assert_eq!(check_code(&seed, &code(&seed, now, 6), now, None), Some(now));
        assert_eq!(check_code(&seed, &code(&seed, now - 1, 6), now, None), Some(now - 1), "a step late");
        assert_eq!(check_code(&seed, &code(&seed, now + 1, 6), now, None), Some(now + 1), "a step early");
        assert_eq!(check_code(&seed, &code(&seed, now - 2, 6), now, None), None, "two steps is too far");
        assert_eq!(check_code(&seed, &code(&seed, now, 6), now, Some(now)), None, "a step once taken");
        assert_eq!(check_code(&seed, &code(&seed, now - 1, 6), now, Some(now - 1)), None);
        assert_eq!(check_code(&seed, " 12a456", now, None), None);
        assert_eq!(check_code(&seed, "", now, None), None);
    }

    #[test]
    fn digits_are_digits_each_as_likely_as_the_next() {
        assert_eq!(digits(6).len(), 6);
        let mut counts = [0u32; 10];
        let n = 200_000;
        for c in digits(n).bytes() {
            counts[(c - b'0') as usize] += 1;
        }
        // Chi-squared over 9 degrees of freedom: about 9 when even; a
        // `% 10` bias over 200,000 digits makes it about 70.
        let expect = n as f64 / 10.0;
        let chi: f64 = counts.iter().map(|&c| (c as f64 - expect).powi(2) / expect).sum();
        assert!(chi < 40.0, "{counts:?}, chi-squared {chi}");
    }

    #[test]
    fn a_password_checks_against_its_own_hash_only() {
        let hash = hash_password("correct horse battery");
        assert!(hash.starts_with("$argon2id$"));
        assert!(check_password("correct horse battery", Some(&hash)));
        assert!(!check_password("correct horse batterY", Some(&hash)));
        assert!(!check_password("nobody's password", None), "no member, no way in, whatever is typed");
    }

    #[test]
    fn a_seal_opens_for_its_member_under_its_key_and_for_no_one_else() {
        let keys = Keys::from_hex(&"ab".repeat(32)).unwrap();
        let sealed = keys.seal(7, b"seed bytes");
        assert_eq!(keys.open(7, &sealed).as_deref(), Some(&b"seed bytes"[..]));
        assert_eq!(keys.open(8, &sealed), None, "moved to another member's row");
        assert_eq!(Keys::from_hex(&"cd".repeat(32)).unwrap().open(7, &sealed), None, "another key");
        let mut touched = sealed.clone();
        touched[30] ^= 1;
        assert_eq!(keys.open(7, &touched), None);
        assert_ne!(keys.seal(7, b"seed bytes"), sealed, "a nonce of its own each time");
        assert!(Keys::from_hex("abcd").is_err());
    }

    #[test]
    fn hashes_are_keyed_and_kept_apart_by_purpose() {
        let keys = Keys::from_hex(&"ab".repeat(32)).unwrap();
        assert_eq!(keys.hash("verify", "123456"), keys.hash("verify", "123456"));
        assert_ne!(keys.hash("verify", "123456"), keys.hash("reset", "123456"));
        assert_ne!(keys.hash("verify", "123456"), Keys::from_hex(&"cd".repeat(32)).unwrap().hash("verify", "123456"));
    }

    #[test]
    fn recovery_codes_are_two_fives_of_letters_hard_to_misread() {
        let code = recovery_code();
        assert_eq!(code.len(), 11);
        assert_eq!(&code[5..6], "-");
        assert!(code.chars().all(|c| c == '-' || "abcdefghjkmnpqrstuvwxyz23456789".contains(c)), "{code}");
        assert_eq!(plain_recovery_code(" ABCDE-fgh2k "), "abcdefgh2k");
        assert_ne!(recovery_code(), recovery_code());
    }

    #[test]
    fn the_seed_address_names_the_board_and_the_member() {
        let uri = seed_uri(b"12345678901234567890", "Anie Solutions BBS", "bryan");
        assert_eq!(uri, "otpauth://totp/Anie%20Solutions%20BBS:bryan?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Anie%20Solutions%20BBS&algorithm=SHA1&digits=6&period=30");
    }
}
