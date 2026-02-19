<p align="center"><img src="logo.svg" width="80" /></p>

# Prisoner's Arena Program

**Competitive AI Tournament on Solana**

Players stake SOL, select from built-in strategies or author custom bytecode programs, and compete in automated Iterated Prisoner's Dilemma matches for a share of the prize pool.

[prisoners-arena.com](https://prisoners-arena.com)

## Table of Contents

- [The Game](#the-game)
- [Verifying the On-Chain Program](#verifying-the-on-chain-program)
- [Architecture](#architecture)
  - [Smart Contract](#smart-contract-programsprisoners-arena)
  - [Match Logic](#match-logic-cratesmatch-logic)
  - [Custom Strategy VM](#custom-strategy-vm-cratesmatch-logicvmrs)
  - [Tournament State Machine](#tournament-state-machine)
- [Building and Testing](#building-and-testing)
  - [Prerequisites](#prerequisites)
  - [Build](#build)
  - [Test](#test)
  - [Testing Custom Strategies](#testing-custom-strategies)
  - [Format and Lint](#format-and-lint)
- [Key Design Decisions](#key-design-decisions)
- [Links](#links)
- [License](#license)

## The Game

The Prisoner's Dilemma is a game theory scenario where two players independently choose to cooperate or defect. The rational choice for each individual is to defect, yet mutual cooperation yields a better collective outcome. In an iterated tournament, strategies that build trust and retaliate against exploitation tend to outperform pure defection.

### Payoff Matrix

| | Cooperate | Defect |
|---|---|---|
| **Cooperate** | 3, 3 | 0, 5 |
| **Defect** | 5, 0 | 1, 1 |

For full rules, available strategies, and tournament lifecycle details, see [How It Works](https://prisoners-arena.com/docs).

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

A pure, dependency-minimal crate that implements strategy behavior, match execution, round-robin pairing, and seeded RNG. Deterministic by design: given the same inputs (strategies, seed), every execution produces identical results. This makes matches independently verifiable.

### Custom Strategy VM (`crates/match-logic/vm.rs`)

Players can author custom strategies as compact bytecode programs (up to 64 bytes), interpreted on-chain within the match execution pipeline. The stack-based VM has 25 opcodes, 128-instruction fuel limit per round, and fails safe to Cooperate on any error. Strategy index 9 selects Custom; builtins (0–8) remain as native optimized code paths with zero performance regression.

### Tournament State Machine

```
Registration → Reveal → Running → Payout
```

- **Registration** — Players submit a commitment hash and stake SOL.
- **Reveal** — Players disclose their strategy, salt, and bytecode (for custom strategies). The contract verifies each reveal against its commitment.
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

### Testing Custom Strategies

Add `match-logic` as a dependency to test custom bytecode programs locally. The same code that runs on-chain executes on your machine — results are deterministic given the same seed.

```rust
use match_logic::{validate_bytecode, run_match, PlayerStrategy, Strategy, StrategyBase};

fn main() {
    // TitForTat as bytecode: OPP_LAST RETURN
    let bytecode = vec![0x02, 0x18];

    // Validate before submitting on-chain
    validate_bytecode(&bytecode).expect("invalid program");

    // Test against a builtin strategy
    let custom   = PlayerStrategy::Custom(bytecode);
    let defector = PlayerStrategy::Builtin(Strategy::new(StrategyBase::AlwaysDefect));

    let seed = [0u8; 32];
    let result = run_match(&custom, &defector, &seed, 0, 8);

    println!("Custom: {} | Defector: {}", result.total_score_a, result.total_score_b);
    for r in &result.rounds {
        println!("  R{}: {:?} vs {:?} -> {}-{}",
            r.round, r.move_a, r.move_b, r.score_a, r.score_b);
    }
}
```

`validate_bytecode()` runs the same 6 checks performed on-chain during reveal. `run_match()` executes a full match with round-by-round scores. See the [VM specification](https://prisoners-arena.com/docs/custom-strategy-vm) for the complete opcode reference.

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
- [How It Works](https://prisoners-arena.com/docs) — Rules, strategies, and tournament lifecycle
- [Custom Strategy VM](https://prisoners-arena.com/docs/custom-strategy-vm) — Bytecode VM specification for custom strategies
- [API Documentation](https://prisoners-arena.com/api) — REST API for querying on-chain state

## License

Copyright (c) 2026 Prisoners Arena Contributors. Licensed under the [PolyForm Noncommercial 1.0.0](LICENSE).
