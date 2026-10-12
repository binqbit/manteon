//! Canonical human-readable result bytes shared by the program and its clients.
use anchor_lang::prelude::*;
use core::fmt::Write;

use crate::error::ManteonError;

/// Last second representable by the four-digit UTC calendar format.
pub const MAX_TIMESTAMP: i64 = 253_402_300_799;

/// Gregorian civil date conversion, bounded to 1970 through 9999, with constant work.
pub fn timestamp(value: i64) -> Result<String> {
    require!(
        (0..=MAX_TIMESTAMP).contains(&value),
        ManteonError::InvalidEventResult
    );
    // March-based Gregorian eras: 400 years contain exactly 146097 days.
    let days = value / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = march_month + if march_month < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let seconds = value % 86_400;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    ))
}

/// Shared time and envelope; event definitions validate their additional facts.
pub(crate) fn render(
    pool: Pubkey,
    agreement_hash: &[u8; 32],
    event_kind: &str,
    occurred_at: i64,
    facts: &str,
) -> Result<String> {
    require!(*agreement_hash != [0; 32], ManteonError::InvalidAgreement);
    let mut agreement = String::with_capacity(64);
    for byte in agreement_hash {
        write!(&mut agreement, "{byte:02x}").map_err(|_| error!(ManteonError::Arithmetic))?;
    }

    Ok(format!(
        concat!(
            "# Manteon event result\n\n",
            "Version: 1\n\n",
            "## Reported event\n\n",
            "- Event: {event_kind}\n",
            "{facts}- Occurred at: {occurred_at}\n\n",
            "## Agreement and pool\n\n",
            "- Program: {program}\n",
            "- Pool: {pool}\n",
            "- Agreement hash: {agreement}\n",
        ),
        program = crate::ID,
        pool = pool,
        agreement = agreement,
        event_kind = event_kind,
        occurred_at = timestamp(occurred_at)?,
        facts = facts,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_boundaries_are_canonical_and_bounded() {
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"),
            (MAX_TIMESTAMP, "9999-12-31T23:59:59Z"),
        ] {
            assert_eq!(timestamp(seconds).unwrap(), expected);
        }
        assert!(timestamp(-1).is_err());
        assert!(timestamp(MAX_TIMESTAMP + 1).is_err());
    }
}
