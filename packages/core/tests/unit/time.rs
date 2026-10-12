use manteon_core::{
    DomainError,
    constants::SECONDS_PER_DAY,
    math::utc_day::{day_error, day_start, utc_day},
    validation::time::Schedule,
};

fn schedule() -> Schedule {
    Schedule {
        opens_at: 10,
        lock_at: SECONDS_PER_DAY + 1,
        horizon_at: 10 * SECONDS_PER_DAY,
    }
}

#[test]
fn mat_015_creation_and_activation_respect_committed_schedule() {
    let valid = schedule();

    assert_eq!(valid.validate_creation(9), Ok(()));
    assert_eq!(
        valid.validate_creation(10),
        Err(DomainError::InvalidSchedule)
    );
    assert_eq!(valid.validate_activation_time(9), Ok(()));
    assert_eq!(
        valid.validate_activation_time(10),
        Err(DomainError::ActivationTooLate)
    );

    for invalid in [
        Schedule {
            lock_at: 10,
            ..valid
        },
        Schedule {
            horizon_at: 12,
            ..valid
        },
    ] {
        assert_eq!(
            invalid.validate_creation(0),
            Err(DomainError::InvalidSchedule)
        );
        assert_eq!(
            invalid.validate_entry_time(11),
            Err(DomainError::InvalidSchedule)
        );
    }

    assert_eq!(
        Schedule {
            horizon_at: valid.lock_at,
            ..valid
        }
        .validate_creation(0),
        Ok(())
    );
}

#[test]
fn mat_025_global_lock_stops_entry_without_phase_update() {
    let schedule = schedule();

    assert_eq!(
        schedule.validate_entry_time(9),
        Err(DomainError::BeforeOpen)
    );
    assert_eq!(schedule.validate_entry_time(10), Ok(()));
    assert_eq!(schedule.validate_entry_time(schedule.lock_at - 1), Ok(()));
    assert_eq!(
        schedule.validate_entry_time(schedule.lock_at),
        Err(DomainError::Locked)
    );
    assert_eq!(
        schedule.validate_entry_time(i64::MAX),
        Err(DomainError::Locked)
    );
}

#[test]
fn mat_025_midday_lock_requires_the_next_forecast_day() {
    assert_eq!(
        schedule().validate_forecast(1),
        Err(DomainError::InvalidForecastDay)
    );
    assert_eq!(schedule().validate_forecast(2), Ok(()));
    assert_eq!(
        schedule().validate_forecast(i64::MAX),
        Err(DomainError::ArithmeticOverflow)
    );
}

#[test]
fn mat_056_signed_utc_uses_floor_and_checked_boundaries() {
    for (timestamp, expected) in [
        (-86_401, -2),
        (-86_400, -1),
        (-1, -1),
        (0, 0),
        (86_399, 0),
        (86_400, 1),
    ] {
        assert_eq!(utc_day(timestamp), expected);
    }

    for timestamp in [i64::MIN, -1, 0, i64::MAX] {
        let day = utc_day(timestamp);
        let start = i128::from(day) * i128::from(SECONDS_PER_DAY);

        assert!(start <= i128::from(timestamp));
        assert!(i128::from(timestamp) < start + i128::from(SECONDS_PER_DAY));
    }

    assert_eq!(day_start(i64::MAX), Err(DomainError::ArithmeticOverflow));
    assert_eq!(day_start(i64::MIN), Err(DomainError::ArithmeticOverflow));
    assert_eq!(day_error(i64::MIN, i64::MAX), u64::MAX);
    assert_eq!(day_error(-20, 10), day_error(10, -20));
}

#[test]
fn mat_033_horizon_guard_does_not_consult_case_status() {
    let schedule = schedule();

    assert_eq!(
        schedule.validate_horizon_time(schedule.horizon_at - 1),
        Err(DomainError::BeforeHorizon)
    );
    assert_eq!(schedule.validate_horizon_time(schedule.horizon_at), Ok(()));
    assert_eq!(
        schedule.validate_horizon_time(schedule.horizon_at + 1),
        Ok(())
    );
}
