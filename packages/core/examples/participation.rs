//! Synthetic domain calculation example; no wallet, chain or token transfer.

use manteon_core::{
    accounting::{allowance::quote_first_deposit, principal::PrincipalBook},
    constants::WITHDRAWAL_WINDOW_SECONDS,
    types::Amount,
};

fn main() -> Result<(), manteon_core::DomainError> {
    let first = quote_first_deposit(Amount::new(100), 0)?;
    let expired_top_up = first.quote_top_up(Amount::new(20), WITHDRAWAL_WINDOW_SECONDS + 1)?;
    let live_top_up = expired_top_up.quote_top_up(Amount::new(5), WITHDRAWAL_WINDOW_SECONDS + 2)?;
    assert_eq!(live_top_up.principal(), Amount::new(125));
    assert_eq!(live_top_up.withdrawable_principal(), Amount::new(25));
    assert_eq!(
        live_top_up.withdraw_until(),
        expired_top_up.withdraw_until()
    );

    let before = PrincipalBook::new(Amount::new(200), Amount::new(100), 2)?;
    let residual = before.quote_remove(Amount::new(100))?;
    assert_eq!(residual.pool_principal(), Amount::new(100));
    assert_eq!(residual.active_dates(), 1);

    println!("Synthetic allowance: principal=125, withdrawable=25; old 100 stays locked.");
    println!("Residual accounting quote: pool=100, active_dates=1; no transfer performed.");
    Ok(())
}
