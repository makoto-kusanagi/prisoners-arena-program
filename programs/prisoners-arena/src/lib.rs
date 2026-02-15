//! Prisoner's Arena - Iterated Prisoner's Dilemma Tournament
//!
//! A Solana smart contract for running competitive tournaments
//! based on the classic Prisoner's Dilemma game theory scenario.

use anchor_lang::prelude::*;

mod state;
mod instructions;
mod error;

use instructions::*;
pub use state::Strategy;
pub use state::StrategyParams;

declare_id!("89Pm5Qy61r1K8dLY1Z1fsJLu3PBN5tTLfZFoEAhejDYa");

#[program]
pub mod prisoners_arena {
    use super::*;

    /// Initialize the global config and Tournament #0 (one-time setup)
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        params: InitializeConfigParams,
    ) -> Result<()> {
        instructions::admin::initialize_config(ctx, params)
    }

    /// Update config parameters (admin only)
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        params: UpdateConfigParams,
    ) -> Result<()> {
        instructions::admin::update_config(ctx, params)
    }

    /// Withdraw accumulated house fees (admin only)
    pub fn withdraw_fees(ctx: Context<WithdrawFees>) -> Result<()> {
        instructions::admin::withdraw_fees(ctx)
    }

    /// Enter the current tournament with a commitment hash
    pub fn enter_tournament(
        ctx: Context<EnterTournament>,
        commitment: [u8; 32],
    ) -> Result<()> {
        instructions::player::enter_tournament(ctx, commitment)
    }

    /// Reveal strategy during Reveal phase
    pub fn reveal_strategy(
        ctx: Context<RevealStrategy>,
        strategy: state::Strategy,
        params: state::StrategyParams,
        salt: [u8; 16],
    ) -> Result<()> {
        instructions::player::reveal_strategy(ctx, strategy, params, salt)
    }

    /// Claim refund during registration or reveal phase
    pub fn claim_refund(ctx: Context<ClaimRefund>) -> Result<()> {
        instructions::player::claim_refund(ctx)
    }

    /// Claim payout if winner (within 30 days)
    pub fn claim_payout(ctx: Context<ClaimPayout>) -> Result<()> {
        instructions::player::claim_payout(ctx)
    }

    /// Close registration and transition to Reveal phase (or extend deadline)
    pub fn close_registration(ctx: Context<CloseRegistration>) -> Result<()> {
        instructions::tournament::close_registration(ctx)
    }

    /// Close the reveal phase and transition to Running
    pub fn close_reveal(ctx: Context<CloseReveal>) -> Result<()> {
        instructions::tournament::close_reveal(ctx)
    }

    /// Forfeit an unrevealed entry after reveal deadline
    pub fn forfeit_unrevealed(ctx: Context<ForfeitUnrevealed>) -> Result<()> {
        instructions::tournament::forfeit_unrevealed(ctx)
    }

    /// Execute a batch of matches (up to 5 per tx)
    pub fn run_matches<'info>(ctx: Context<'_, '_, '_, 'info, RunMatches<'info>>) -> Result<()> {
        instructions::tournament::run_matches(ctx)
    }

    /// Finalize tournament and create next tournament
    pub fn finalize_tournament(ctx: Context<FinalizeTournament>) -> Result<()> {
        instructions::tournament::finalize_tournament(ctx)
    }

    /// Close entry: distribute payout to winners, return rent to player
    pub fn close_entry(ctx: Context<CloseEntry>) -> Result<()> {
        instructions::tournament::close_entry(ctx)
    }

    /// Close tournament account and recover rent (30 days after payout)
    pub fn close_tournament(ctx: Context<CloseTournament>) -> Result<()> {
        instructions::tournament::close_tournament(ctx)
    }
}
