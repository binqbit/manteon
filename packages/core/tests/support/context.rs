use manteon_core::{accounting::principal::PrincipalBook, types::Amount};

/// A fresh in-memory fixture for pure arithmetic and reproducible time inputs.
///
/// The stored values are synthetic mathematical inputs, never authenticated
/// account state. Updating them cannot serve as evidence of runtime execution.
pub struct TestContext {
    now: i64,
    positions: [u64; 3],
}

impl TestContext {
    /// Creates an isolated synthetic book and an explicit starting timestamp.
    pub const fn new(now: i64, positions: [u64; 3]) -> Self {
        Self { now, positions }
    }

    /// Returns the synthetic timestamp supplied to pure calculations.
    pub const fn now(&self) -> i64 {
        self.now
    }

    /// Sets the synthetic timestamp without claiming to control a runtime Clock.
    pub fn set_time(&mut self, now: i64) {
        self.now = now;
    }

    /// Constructs locally consistent aggregates from the complete tiny fixture.
    pub fn book(&self, position: usize) -> PrincipalBook {
        PrincipalBook::new(
            Amount::new(self.positions.iter().sum()),
            Amount::new(self.positions[position]),
            self.positions.iter().filter(|&&amount| amount > 0).count() as u64,
        )
        .expect("synthetic aggregate must be consistent")
    }

    /// Compares a quote with independently supplied complete resulting amounts.
    pub fn assert_book(quote: PrincipalBook, position: usize, expected: [u64; 3]) {
        let sum: u64 = expected.iter().sum();
        let positive = expected.iter().filter(|&&amount| amount > 0).count() as u64;
        assert_eq!(quote.pool_principal().get(), sum);
        assert_eq!(quote.position_principal().get(), expected[position]);
        assert_eq!(quote.active_dates(), positive);
    }
}
