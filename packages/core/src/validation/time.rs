//! Timeline guards for the committed market schedule.

use crate::{DomainError, math::utc_day::day_start};

/// Committed schedule values, not an onchain Pool layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Schedule {
    /// First possible entry timestamp.
    pub opens_at: i64,
    /// Global stop for deposits and ordinary withdrawals.
    pub lock_at: i64,
    /// Independent exit-time anchor before final confirmation.
    pub horizon_at: i64,
}

impl Schedule {
    /// Check `now < opens_at < lock_at <= horizon_at` at creation.
    ///
    /// A runtime must use executing Clock, authenticate creation authority and
    /// retain these values immutably; this value check cannot do those things.
    pub fn validate_creation(self, now: i64) -> Result<(), DomainError> {
        if now < self.opens_at && self.opens_at < self.lock_at && self.lock_at <= self.horizon_at {
            Ok(())
        } else {
            Err(DomainError::InvalidSchedule)
        }
    }

    /// Activation must precede opening; this grants no activation authority.
    pub fn validate_activation_time(self, now: i64) -> Result<(), DomainError> {
        self.validate_ordering()?;
        if now < self.opens_at {
            Ok(())
        } else {
            Err(DomainError::ActivationTooLate)
        }
    }

    /// Check the inclusive OPEN and exclusive LOCK temporal entry window.
    ///
    /// An authenticated entry-eligible lifecycle phase is separately required.
    pub fn validate_entry_time(self, now: i64) -> Result<(), DomainError> {
        self.validate_ordering()?;
        if now < self.opens_at {
            Err(DomainError::BeforeOpen)
        } else if now >= self.lock_at {
            Err(DomainError::Locked)
        } else {
            Ok(())
        }
    }

    /// Check the agreed minimum date without inventing a maximum forecast date.
    pub fn validate_forecast(self, forecast_day: i64) -> Result<(), DomainError> {
        self.validate_ordering()?;
        if day_start(forecast_day)? >= self.lock_at {
            Ok(())
        } else {
            Err(DomainError::InvalidForecastDay)
        }
    }

    /// Check the inclusive horizon-time requirement.
    ///
    /// A pending audit is deliberately not an input to this temporal guard.
    /// The adapter must additionally check that final confirmation/freeze has
    /// not consumed exit rights and perform one atomic principal transfer.
    pub fn validate_horizon_time(self, now: i64) -> Result<(), DomainError> {
        self.validate_ordering()?;
        if now < self.horizon_at {
            Err(DomainError::BeforeHorizon)
        } else {
            Ok(())
        }
    }

    /// Check immutable timestamp ordering without testing the current Clock.
    pub fn validate_ordering(self) -> Result<(), DomainError> {
        if self.opens_at < self.lock_at && self.lock_at <= self.horizon_at {
            Ok(())
        } else {
            Err(DomainError::InvalidSchedule)
        }
    }
}
