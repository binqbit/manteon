//! Canonical addresses and authenticated account reads for test assertions.
use super::{context::TestContext, setup::config::address};
use anchor_lang::{AccountDeserialize, Owner, prelude::Pubkey, solana_program::program_pack::Pack};
use anchor_spl::token::spl_token;
use manteon::state::{Pool, Position, Stake};
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

impl TestContext {
    pub fn position(&self, day: i64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                Position::SEED,
                self.pool.pubkey().as_ref(),
                &day.to_le_bytes(),
            ],
            &manteon::ID,
        )
        .0
    }

    pub fn stake_key(&self, owner: &Keypair, day: i64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                Stake::SEED,
                self.position(day).as_ref(),
                owner.pubkey().as_ref(),
            ],
            &manteon::ID,
        )
        .0
    }

    pub fn pool_state(&self) -> Pool {
        self.decode(self.pool.pubkey())
    }
    pub fn event_state(&self) -> manteon::state::Event {
        self.decode(address(super::setup::pool::PoolContext::event(&self.pool)))
    }
    pub fn position_state(&self, day: i64) -> Position {
        self.decode(address(self.position(day)))
    }
    pub fn stake_state(&self, owner: &Keypair, day: i64) -> Stake {
        self.decode(address(self.stake_key(owner, day)))
    }

    fn decode<T: AccountDeserialize + Owner>(&self, key: Address) -> T {
        let account = self.svm.get_account(&key).unwrap();
        assert_eq!(account.owner, address(T::owner()));
        assert!(!account.executable);
        T::try_deserialize(&mut account.data.as_slice()).unwrap()
    }

    pub fn token_balance(&self, key: Address) -> u64 {
        let account = self.svm.get_account(&key).unwrap();
        assert_eq!(account.owner, address(spl_token::ID));
        assert!(!account.executable);
        spl_token::state::Account::unpack(&account.data)
            .unwrap()
            .amount
    }

    pub fn policy_uses(&self) -> u64 {
        assert_eq!(
            self.svm
                .get_account(&address(self.policy))
                .unwrap()
                .data
                .len(),
            tekmerion_sdk::program::Policy::SPACE,
        );
        let policy: tekmerion_sdk::program::Policy = self.decode(address(self.policy));
        policy.active_uses
    }
}
