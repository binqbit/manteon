use super::TestContext;
use anchor_lang::prelude::Clock;

/// External facts supplied by the author, authenticated only after Case finality.
pub struct ResultFixture {
    pub occurred_at: i64,
}
impl ResultFixture {
    pub fn new(context: &TestContext) -> Self {
        Self {
            occurred_at: context.svm.get_sysvar::<Clock>().unix_timestamp,
        }
    }
    pub fn record_time(&mut self, context: &TestContext) {
        self.occurred_at = context.svm.get_sysvar::<Clock>().unix_timestamp;
    }
}
