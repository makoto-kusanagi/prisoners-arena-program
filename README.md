<p align="center"><img src="logo.svg" width="80" /></p>

# Prisoner's Arena Program

Competitive AI tournament platform on Solana implementing the Iterated Prisoner's Dilemma. Players stake SOL, select strategies with configurable parameters, compete in automated matches, and split prizes.

[prisoners-arena.com](https://prisoners-arena.com)

## The Game

The Prisoner's Dilemma is a game theory scenario where two players independently choose to cooperate or defect. The rational choice for each individual is to defect, yet mutual cooperation yields a better collective outcome. In an iterated tournament, strategies that build trust and retaliate against exploitation tend to outperform pure defection.

### Payoff Matrix

| | Cooperate | Defect |
|---|---|---|
| **Cooperate** | 3, 3 | 0, 5 |
| **Defect** | 5, 0 | 1, 1 |

For full rules, available strategies, configurable parameters, and tournament lifecycle details, see [How It Works](https://prisoners-arena.com/how-it-works).

## Verifying the On-Chain Program

Anyone can verify that the deployed program matches this source code using [solana-verify](https://github.com/Ellipsis-Labs/solana-verifiable-build).

The program ID can be found via the [config API](https://prisoners-arena.com/api/config) in the `data.programId` field.

```bash
solana-verify verify-from-repo \
    https://github.com/makoto-kusanagi/prisoners-arena-program \
    --program-id <PROGRAM_ID> \
    --library-name prisoners_arena
```

## Architecture

### Smart Contract (`programs/prisoners-arena`)

The Anchor program manages tournament state, player entries, and fund custody. Three PDA account types track global configuration, per-tournament state, and per-player entries. All funds are held in program-derived accounts — no external custody.

### Match Logic (`crates/match-logic`)

A pure, dependency-minimal crate that implements strategy behavior, match execution, round-robin pairing, and seeded RNG. Deterministic by design: given the same inputs (strategies, parameters, seed), every execution produces identical results. This makes matches independently verifiable.

### Tournament State Machine

```
Registration → Reveal → Running → Payout
```

- **Registration** — Players submit a commitment hash and stake SOL.
- **Reveal** — Players disclose their strategy, params, and salt. The contract verifies each reveal against its commitment.
- **Running** — An operator executes matches in batches. All match logic runs on-chain using the shared crate.
- **Payout** — Top 25% of players by score are winners. Winners claim their share of the prize pool.

## Building and Testing

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Solana CLI](https://docs.solanalabs.com/cli/install) (v2.0+)
- [Anchor CLI](https://www.anchor-lang.com/docs/installation) (v0.32)

### Build

```bash
# Build the Anchor program
anchor build

# Build match-logic for WASM (requires wasm-pack)
cd crates/match-logic
wasm-pack build --target web -- --features wasm
```

### Test

```bash
# Unit tests for match logic
cargo test -p match-logic

# Integration tests (requires local validator)
anchor test --provider.cluster localnet -- --features testing
```

### Format and Lint

```bash
cargo fmt
cargo clippy
```

## Key Design Decisions

- **Commit-reveal** — Players commit a hash of their strategy before revealing, preventing opponents from observing and countering choices.
- **Config snapshotting** — Tournament parameters (stake amount, fees, timing) are captured at creation time, isolating in-progress tournaments from config changes.
- **Dynamic realloc** — Tournament accounts grow via `realloc` as participants join, avoiding fixed-size allocation limits.
- **Operator reimbursement** — The operator is reimbursed for transaction costs from the tournament's fee pool, making automation economically sustainable.
- **Deterministic execution** — Match logic uses a seeded RNG derived from on-chain data, ensuring reproducible results across contract, operator, and frontend.
- **Variable-length rounds** — Round count per match is determined by a configurable range, adding strategic depth without sacrificing determinism.

## Links

- [prisoners-arena.com](https://prisoners-arena.com)
- [How It Works](https://prisoners-arena.com/how-it-works) — Rules, strategies, parameters, and tournament lifecycle
- [API Documentation](https://prisoners-arena.com/api) — REST API for querying on-chain state

## License

Copyright 2025 Prisoner's Arena contributors. Source code is publicly available for verification and auditing purposes. Not licensed for commercial use, redistribution, or derivative works.
