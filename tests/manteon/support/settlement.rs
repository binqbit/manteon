use super::{
    context::{AUDIT_MINT, TEKMERION, TestContext},
    result::ResultFixture,
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
};
use anchor_lang::{InstructionData, ToAccountMetas, prelude::Pubkey};
use anchor_spl::token::spl_token;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tekmerion_sdk::instructions::{self, CaseAddress};

pub struct CaseFixture {
    pub identity: CaseAddress,
    pub key: Pubkey,
    pub vault: Pubkey,
}
impl CaseFixture {
    pub fn instruction(context: &TestContext, result: &ResultFixture) -> (Self, Instruction) {
        Self::instruction_for(
            context,
            result,
            pubkey(context.alice.pubkey()),
            pubkey(context.audit_source),
        )
    }

    pub fn instruction_for(
        context: &TestContext,
        result: &ResultFixture,
        author: Pubkey,
        source: Pubkey,
    ) -> (Self, Instruction) {
        let nonce = context.pool_state().next_case_nonce;
        let identity = CaseAddress {
            policy: context.policy,
            subject: pubkey(context.pool.pubkey()),
            nonce,
        };
        let key = identity.address();
        let vault = identity.vault();
        let ix = Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::ProposeCompanyIpo {
                author,
                payer: pubkey(context.payer.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                policy: context.policy,
                client_authority: context.client_authority,
                source,
                case: key,
                author_receipt: identity.receipt(&author),
                case_vault: vault,
                bond_mint: AUDIT_MINT,
                tekmerion_program: TEKMERION,
                token_program: spl_token::ID,
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::ProposeCompanyIpo {
                data: manteon::state::EventResult {
                    extra: None,
                    occurred_at: result.occurred_at,
                },
            }
            .data(),
        };
        (
            Self {
                identity,
                key,
                vault,
            },
            ix,
        )
    }
    pub fn start(context: &mut TestContext, result: &ResultFixture) -> Self {
        let (case, ix) = Self::instruction(context, result);
        let author = context.alice.insecure_clone();
        let before = context.token_balance(context.audit_source);
        context.send(&[ix], &[&author]);
        assert_eq!(context.token_balance(context.audit_source), before - 100);
        assert_eq!(context.token_balance(address(case.vault)), 100);
        assert_eq!(context.pool_state().current_case, Some(case.key));
        case
    }
    pub fn finalize_yes(&self, context: &mut TestContext) {
        let case = super::tekmerion::case_state(context, self);
        context.set_time(case.funding_deadline);
        context.send(&[instructions::finalize_unchallenged(self.identity)], &[]);
    }

    pub fn begin_ix(&self, context: &TestContext, caller: &Keypair) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::BeginSettlement {
                caller: pubkey(caller.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                case: Some(self.key),
                vault: PoolContext::vault(&context.pool),
            }
            .to_account_metas(None),
            data: manteon::instruction::BeginSettlement {}.data(),
        }
    }
    pub fn process_ix(&self, context: &TestContext, caller: &Keypair, days: &[i64]) -> Instruction {
        let mut accounts = manteon::accounts::ProcessPositions {
            caller: pubkey(caller.pubkey()),
            config: pubkey(context.config_address),
            pool: pubkey(context.pool.pubkey()),
            event: PoolContext::event(&context.pool),
            case: Some(self.key),
        }
        .to_account_metas(None);
        accounts.extend(
            days.iter()
                .map(|day| AccountMeta::new(address(context.position(*day)), false)),
        );
        Instruction {
            program_id: address(manteon::ID),
            accounts,
            data: manteon::instruction::ProcessPositions {}.data(),
        }
    }

    pub fn finalize_ix(&self, context: &TestContext, caller: &Keypair) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::FinalizeSettlement {
                caller: pubkey(caller.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                case: Some(self.key),
            }
            .to_account_metas(None),
            data: manteon::instruction::FinalizeSettlement {}.data(),
        }
    }
}
pub fn claim_ix(
    context: &TestContext,
    owner: &Keypair,
    day: i64,
    destination: Pubkey,
) -> Instruction {
    Instruction {
        program_id: address(manteon::ID),
        accounts: manteon::accounts::Claim {
            config: pubkey(context.config_address),
            fee_recipient: Some(pubkey(context.fee_recipient)),
            platform_fee_vault: manteon::state::GlobalConfig::vault_address(&pubkey(context.mint)),
            owner: pubkey(owner.pubkey()),
            pool: pubkey(context.pool.pubkey()),
            event: PoolContext::event(&context.pool),
            position: context.position(day),
            stake: context.stake_key(owner, day),
            collateral_mint: pubkey(context.mint),
            destination,
            vault_authority: PoolContext::vault_authority(&context.pool),
            vault: PoolContext::vault(&context.pool),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: manteon::instruction::Claim {}.data(),
    }
}
