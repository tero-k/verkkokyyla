//! Shared local backup filename validation (MikroTik + OPNsense backups).
//!
//! Validates a user-supplied backup name BEFORE any device call or local
//! file creation: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` and the stem before
//! the first dot, case-insensitively, must not be a Windows device name.

use std::fmt;

/// `backup_name` violates the name rule or is a Windows device name.
/// Carries the offending name; callers map this into their own typed error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvalidBackupName(pub String);

impl fmt::Display for InvalidBackupName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid backup name {:?}: must match ^[A-Za-z0-9][A-Za-z0-9._-]{{0,63}}$ \
             and must not be a Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)",
            self.0
        )
    }
}

impl std::error::Error for InvalidBackupName {}

/// Validate `backup_name`: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` and the stem
/// before the first dot, case-insensitively, must not be a Windows device
/// name (so `CON`, `con.txt`, `Com1.backup` all reject).
pub(crate) fn validate_backup_name(name: &str) -> Result<(), InvalidBackupName> {
    let mut chars = name.chars();
    let first = chars
        .next()
        .ok_or_else(|| InvalidBackupName(name.to_owned()))?;
    let valid = first.is_ascii_alphanumeric()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(InvalidBackupName(name.to_owned()));
    }
    let stem = name.split('.').next().unwrap_or(name);
    if is_windows_device_name(stem) {
        return Err(InvalidBackupName(name.to_owned()));
    }
    Ok(())
}

/// CON, PRN, AUX, NUL, COM1-9, LPT1-9 (case-insensitive; COM0/LPT0 are
/// NOT reserved device names).
fn is_windows_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_name_validation_table_reserved_set() {
        // Full Windows device set: CON, PRN, AUX, NUL, COM1-9, LPT1-9 —
        // mixed case and dotted extensions must all reject.
        let mut reserved: Vec<String> = vec![
            "CON".into(),
            "con.txt".into(),
            "Con".into(),
            "cOn.bAk".into(),
            "PRN".into(),
            "prn.log".into(),
            "AUX".into(),
            "aux.txt".into(),
            "NUL".into(),
            "nul.rsc".into(),
        ];
        for n in 1..=9 {
            reserved.push(format!("COM{n}"));
            reserved.push(format!("Com{n}.backup"));
            reserved.push(format!("lpt{n}"));
            reserved.push(format!("LPT{n}.x"));
        }
        for name in reserved {
            assert!(
                validate_backup_name(&name).is_err(),
                "{name:?} must be rejected"
            );
        }
        // COM0/LPT0/COM10 are NOT reserved; ordinary names are fine.
        for name in [
            "demo",
            "verkkokyyla-20260101-120000",
            "a.b.c",
            "COM0",
            "LPT0",
            "COM10",
            "LPT10",
            "conx",
            "console",
            "ok_1.2",
        ] {
            assert!(
                validate_backup_name(name).is_ok(),
                "{name:?} must be accepted"
            );
        }
        // Regex violations: empty, bad first char, illegal chars, >64 chars.
        // `_ok-1.2` starts with `_`: the spec regex requires an alphanumeric
        // FIRST char (`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`), so it rejects too.
        for name in [
            "",
            ".hidden",
            "-dash",
            "_ok-1.2",
            "a b",
            "a/b",
            "a\\b",
            &"x".repeat(65),
        ] {
            assert!(
                validate_backup_name(name).is_err(),
                "{name:?} must be rejected"
            );
        }
    }
}
