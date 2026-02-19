//! Stack-based bytecode VM for custom player strategies.
//!
//! Programs are at most 64 bytes and run on a fixed 8-slot u8 stack.
//! Any error (stack underflow, unknown opcode, fuel exhaustion) defaults
//! to Cooperate — the "fail-safe" rule.

use crate::random::SeededRng;
use crate::strategy::Move;
use crate::payoff;

// ── Constants ────────────────────────────────────────────────────────

/// Maximum bytecode program length in bytes.
pub const MAX_BYTECODE_LEN: usize = 64;

/// Maximum instructions the VM will execute before halting (fuel limit).
const MAX_FUEL: u32 = 128;

/// Stack depth (fixed array, no heap).
const STACK_SIZE: usize = 8;

// ── Opcodes ──────────────────────────────────────────────────────────

pub mod op {
    pub const COOP: u8 = 0x00;
    pub const PUSH: u8 = 0x01;
    pub const OPP_LAST: u8 = 0x02;
    pub const MY_LAST: u8 = 0x03;
    pub const OPP_N: u8 = 0x04;
    pub const MY_N: u8 = 0x05;
    pub const OPP_DEFECTS: u8 = 0x06;
    pub const MY_DEFECTS: u8 = 0x07;
    pub const ROUND: u8 = 0x08;
    pub const RAND: u8 = 0x09;
    pub const ADD: u8 = 0x0A;
    pub const SUB: u8 = 0x0B;
    pub const MUL: u8 = 0x0C;
    pub const GT: u8 = 0x0D;
    pub const LT: u8 = 0x0E;
    pub const EQ: u8 = 0x0F;
    pub const NOT: u8 = 0x10;
    pub const AND: u8 = 0x11;
    pub const OR: u8 = 0x12;
    pub const DUP: u8 = 0x13;
    pub const JMP_FWD: u8 = 0x14;
    pub const JMP_FWD_IF: u8 = 0x15;
    pub const DEFECT: u8 = 0x16;
    pub const SCORE_LAST: u8 = 0x17;
    pub const RETURN: u8 = 0x18;
}

// ── Validation ───────────────────────────────────────────────────────

/// Errors that can occur during bytecode validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodeError {
    /// Program is empty.
    Empty,
    /// Program exceeds MAX_BYTECODE_LEN bytes.
    TooLong,
    /// Unknown opcode encountered at the given offset.
    UnknownOpcode { offset: usize, opcode: u8 },
    /// An instruction with an immediate operand is truncated.
    TruncatedImmediate { offset: usize },
    /// A forward jump lands out of bounds.
    JumpOutOfBounds { offset: usize },
    /// Program has no reachable terminal instruction (COOP/DEFECT/RETURN).
    NoTerminal,
}

impl core::fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BytecodeError::Empty => write!(f, "bytecode is empty"),
            BytecodeError::TooLong => write!(f, "bytecode exceeds {} bytes", MAX_BYTECODE_LEN),
            BytecodeError::UnknownOpcode { offset, opcode } =>
                write!(f, "unknown opcode 0x{:02X} at offset {}", opcode, offset),
            BytecodeError::TruncatedImmediate { offset } =>
                write!(f, "truncated immediate at offset {}", offset),
            BytecodeError::JumpOutOfBounds { offset } =>
                write!(f, "forward jump out of bounds at offset {}", offset),
            BytecodeError::NoTerminal =>
                write!(f, "no terminal instruction (COOP/DEFECT/RETURN)"),
        }
    }
}

/// Validate bytecode before storing on-chain.
///
/// Checks:
/// - Non-empty, at most `MAX_BYTECODE_LEN` bytes
/// - All opcodes are known
/// - All immediates are present (not truncated)
/// - All forward jumps land within bounds
/// - At least one terminal instruction exists
pub fn validate_bytecode(bytecode: &[u8]) -> Result<(), BytecodeError> {
    if bytecode.is_empty() {
        return Err(BytecodeError::Empty);
    }
    if bytecode.len() > MAX_BYTECODE_LEN {
        return Err(BytecodeError::TooLong);
    }

    let mut pc = 0usize;
    let mut has_terminal = false;

    while pc < bytecode.len() {
        let opcode = bytecode[pc];
        match opcode {
            // 1-byte terminals
            op::COOP | op::DEFECT => {
                has_terminal = true;
                pc += 1;
            }
            // 1-byte terminal (pops stack)
            op::RETURN => {
                has_terminal = true;
                pc += 1;
            }
            // 2-byte: opcode + immediate
            op::PUSH => {
                if pc + 1 >= bytecode.len() {
                    return Err(BytecodeError::TruncatedImmediate { offset: pc });
                }
                pc += 2;
            }
            // 2-byte: opcode + forward offset
            op::JMP_FWD | op::JMP_FWD_IF => {
                if pc + 1 >= bytecode.len() {
                    return Err(BytecodeError::TruncatedImmediate { offset: pc });
                }
                let offset = bytecode[pc + 1] as usize;
                let target = pc + 2 + offset;
                if target > bytecode.len() {
                    return Err(BytecodeError::JumpOutOfBounds { offset: pc });
                }
                pc += 2;
            }
            // 1-byte instructions
            op::OPP_LAST | op::MY_LAST | op::OPP_N | op::MY_N |
            op::OPP_DEFECTS | op::MY_DEFECTS | op::ROUND | op::RAND |
            op::ADD | op::SUB | op::MUL | op::GT | op::LT | op::EQ |
            op::NOT | op::AND | op::OR | op::DUP | op::SCORE_LAST => {
                pc += 1;
            }
            _ => {
                return Err(BytecodeError::UnknownOpcode { offset: pc, opcode });
            }
        }
    }

    if !has_terminal {
        return Err(BytecodeError::NoTerminal);
    }

    Ok(())
}

// ── Execution ────────────────────────────────────────────────────────

/// Execute a bytecode program and return the chosen move.
///
/// Fail-safe: any runtime error → Cooperate.
pub fn execute_bytecode(
    bytecode: &[u8],
    opponent_history: &[Move],
    my_history: &[Move],
    round: u8,
    rng: &mut SeededRng,
) -> Move {
    execute_inner(bytecode, opponent_history, my_history, round, rng)
        .unwrap_or(Move::Cooperate)
}

/// Inner execution that can fail (returns None on any error).
fn execute_inner(
    bytecode: &[u8],
    opponent_history: &[Move],
    my_history: &[Move],
    round: u8,
    rng: &mut SeededRng,
) -> Option<Move> {
    let mut stack = [0u8; STACK_SIZE];
    let mut sp: usize = 0; // stack pointer (next empty slot)
    let mut pc: usize = 0;
    let mut fuel: u32 = 0;

    while pc < bytecode.len() {
        fuel += 1;
        if fuel > MAX_FUEL {
            return None; // fuel exhaustion → fail-safe
        }

        let opcode = bytecode[pc];
        match opcode {
            op::COOP => return Some(Move::Cooperate),
            op::DEFECT => return Some(Move::Defect),

            op::RETURN => {
                let v = pop(&mut stack, &mut sp)?;
                return Some(if v == 0 { Move::Cooperate } else { Move::Defect });
            }

            op::PUSH => {
                let imm = *bytecode.get(pc + 1)?;
                push(&mut stack, &mut sp, imm)?;
                pc += 2;
            }

            op::OPP_LAST => {
                let v = move_to_u8(opponent_history.last());
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            op::MY_LAST => {
                let v = move_to_u8(my_history.last());
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            op::OPP_N => {
                let n = pop(&mut stack, &mut sp)? as usize;
                let v = history_n_ago(opponent_history, n);
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            op::MY_N => {
                let n = pop(&mut stack, &mut sp)? as usize;
                let v = history_n_ago(my_history, n);
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            op::OPP_DEFECTS => {
                let count = count_defects(opponent_history);
                push(&mut stack, &mut sp, count)?;
                pc += 1;
            }

            op::MY_DEFECTS => {
                let count = count_defects(my_history);
                push(&mut stack, &mut sp, count)?;
                pc += 1;
            }

            op::ROUND => {
                push(&mut stack, &mut sp, round)?;
                pc += 1;
            }

            op::RAND => {
                let v = rng.next_percent();
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            op::ADD => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, a.saturating_add(b))?;
                pc += 1;
            }

            op::SUB => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, a.saturating_sub(b))?;
                pc += 1;
            }

            op::MUL => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, a.saturating_mul(b))?;
                pc += 1;
            }

            op::GT => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a > b))?;
                pc += 1;
            }

            op::LT => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a < b))?;
                pc += 1;
            }

            op::EQ => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a == b))?;
                pc += 1;
            }

            op::NOT => {
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a == 0))?;
                pc += 1;
            }

            op::AND => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a != 0 && b != 0))?;
                pc += 1;
            }

            op::OR => {
                let b = pop(&mut stack, &mut sp)?;
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, u8::from(a != 0 || b != 0))?;
                pc += 1;
            }

            op::DUP => {
                let a = pop(&mut stack, &mut sp)?;
                push(&mut stack, &mut sp, a)?;
                push(&mut stack, &mut sp, a)?;
                pc += 1;
            }

            op::JMP_FWD => {
                let offset = *bytecode.get(pc + 1)? as usize;
                pc = pc + 2 + offset;
            }

            op::JMP_FWD_IF => {
                let cond = pop(&mut stack, &mut sp)?;
                let offset = *bytecode.get(pc + 1)? as usize;
                if cond != 0 {
                    pc = pc + 2 + offset;
                } else {
                    pc += 2;
                }
            }

            op::SCORE_LAST => {
                let v = if my_history.is_empty() {
                    3 // default payoff for round 0 (mutual cooperation)
                } else {
                    let my_last = *my_history.last().unwrap();
                    let opp_last = *opponent_history.last().unwrap_or(&Move::Cooperate);
                    let (score, _) = payoff(my_last, opp_last);
                    score
                };
                push(&mut stack, &mut sp, v)?;
                pc += 1;
            }

            _ => return None, // unknown opcode → fail-safe
        }
    }

    // Fell off the end without a terminal → fail-safe
    None
}

// ── Stack helpers ────────────────────────────────────────────────────

#[inline]
fn push(stack: &mut [u8; STACK_SIZE], sp: &mut usize, val: u8) -> Option<()> {
    if *sp >= STACK_SIZE {
        return None; // overflow
    }
    stack[*sp] = val;
    *sp += 1;
    Some(())
}

#[inline]
fn pop(stack: &mut [u8; STACK_SIZE], sp: &mut usize) -> Option<u8> {
    if *sp == 0 {
        return None; // underflow
    }
    *sp -= 1;
    Some(stack[*sp])
}

// ── History helpers ──────────────────────────────────────────────────

#[inline]
fn move_to_u8(m: Option<&Move>) -> u8 {
    match m {
        Some(Move::Defect) => 1,
        _ => 0, // Cooperate or no history
    }
}

#[inline]
fn history_n_ago(history: &[Move], n: usize) -> u8 {
    if n >= history.len() {
        return 0; // no data → Cooperate
    }
    let idx = history.len() - 1 - n;
    match history[idx] {
        Move::Cooperate => 0,
        Move::Defect => 1,
    }
}

#[inline]
fn count_defects(history: &[Move]) -> u8 {
    let count = history.iter().filter(|m| **m == Move::Defect).count();
    count.min(255) as u8
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> SeededRng {
        SeededRng::new(&[42u8; 32], 0)
    }

    // -- Terminals --

    #[test]
    fn test_coop() {
        let code = [op::COOP];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_defect() {
        let code = [op::DEFECT];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_return_zero_is_cooperate() {
        let code = [op::PUSH, 0, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_return_nonzero_is_defect() {
        let code = [op::PUSH, 1, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    // -- TitForTat as bytecode: OPP_LAST RETURN --

    #[test]
    fn test_bytecode_tit_for_tat() {
        let code = [op::OPP_LAST, op::RETURN];

        // Round 0: no history → cooperate
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);

        // Opponent cooperated → cooperate
        assert_eq!(
            execute_bytecode(&code, &[Move::Cooperate], &[Move::Cooperate], 1, &mut rng()),
            Move::Cooperate,
        );

        // Opponent defected → defect
        assert_eq!(
            execute_bytecode(&code, &[Move::Defect], &[Move::Cooperate], 1, &mut rng()),
            Move::Defect,
        );
    }

    // -- AlwaysDefect as bytecode --

    #[test]
    fn test_bytecode_always_defect() {
        let code = [op::DEFECT];
        for round in 0..10 {
            assert_eq!(execute_bytecode(&code, &[], &[], round, &mut rng()), Move::Defect);
        }
    }

    // -- GrimTrigger as bytecode: OPP_DEFECTS PUSH 0 GT JMP_FWD_IF 1 COOP DEFECT --

    #[test]
    fn test_bytecode_grim_trigger() {
        let code = [
            op::OPP_DEFECTS,    // push count
            op::PUSH, 0,        // push 0
            op::GT,             // count > 0?
            op::JMP_FWD_IF, 1,  // if yes, skip 1 byte
            op::COOP,           // cooperate
            op::DEFECT,         // defect
        ];

        // No defections → cooperate
        assert_eq!(
            execute_bytecode(&code, &[Move::Cooperate, Move::Cooperate], &[], 2, &mut rng()),
            Move::Cooperate,
        );

        // Opponent defected → defect
        assert_eq!(
            execute_bytecode(&code, &[Move::Cooperate, Move::Defect], &[], 2, &mut rng()),
            Move::Defect,
        );
    }

    // -- Arithmetic --

    #[test]
    fn test_add_saturating() {
        let code = [op::PUSH, 200, op::PUSH, 100, op::ADD, op::RETURN];
        // 200 + 100 saturates to 255, nonzero → defect
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_sub_saturating() {
        let code = [op::PUSH, 5, op::PUSH, 10, op::SUB, op::RETURN];
        // 5 - 10 saturates to 0 → cooperate
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_mul_saturating() {
        let code = [op::PUSH, 20, op::PUSH, 20, op::MUL, op::RETURN];
        // 20 * 20 = 400, saturates to 255 → defect
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    // -- Comparison --

    #[test]
    fn test_gt() {
        // 5 > 3 = 1
        let code = [op::PUSH, 5, op::PUSH, 3, op::GT, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);

        // 3 > 5 = 0
        let code = [op::PUSH, 3, op::PUSH, 5, op::GT, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_lt() {
        // 3 < 5 = 1
        let code = [op::PUSH, 3, op::PUSH, 5, op::LT, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_eq() {
        // 5 == 5 = 1
        let code = [op::PUSH, 5, op::PUSH, 5, op::EQ, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);

        // 5 == 3 = 0
        let code = [op::PUSH, 5, op::PUSH, 3, op::EQ, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    // -- Logic --

    #[test]
    fn test_not() {
        let code = [op::PUSH, 0, op::NOT, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);

        let code = [op::PUSH, 1, op::NOT, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_and() {
        let code = [op::PUSH, 1, op::PUSH, 1, op::AND, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);

        let code = [op::PUSH, 1, op::PUSH, 0, op::AND, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_or() {
        let code = [op::PUSH, 0, op::PUSH, 0, op::OR, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);

        let code = [op::PUSH, 1, op::PUSH, 0, op::OR, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    // -- DUP --

    #[test]
    fn test_dup() {
        let code = [op::PUSH, 5, op::DUP, op::EQ, op::RETURN];
        // 5 == 5 → 1 → defect
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    // -- ROUND --

    #[test]
    fn test_round() {
        let code = [op::ROUND, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
        assert_eq!(execute_bytecode(&code, &[], &[], 5, &mut rng()), Move::Defect);
    }

    // -- OPP_N / MY_N --

    #[test]
    fn test_opp_n() {
        let opp = [Move::Cooperate, Move::Defect, Move::Cooperate];
        // n=0 → most recent → Cooperate → 0
        let code = [op::PUSH, 0, op::OPP_N, op::RETURN];
        assert_eq!(execute_bytecode(&code, &opp, &[], 3, &mut rng()), Move::Cooperate);

        // n=1 → one ago → Defect → 1
        let code = [op::PUSH, 1, op::OPP_N, op::RETURN];
        assert_eq!(execute_bytecode(&code, &opp, &[], 3, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_my_n() {
        let my = [Move::Defect, Move::Cooperate];
        // n=0 → most recent → Cooperate → 0
        let code = [op::PUSH, 0, op::MY_N, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &my, 2, &mut rng()), Move::Cooperate);

        // n=1 → one ago → Defect → 1
        let code = [op::PUSH, 1, op::MY_N, op::RETURN];
        assert_eq!(execute_bytecode(&code, &[], &my, 2, &mut rng()), Move::Defect);
    }

    // -- OPP_DEFECTS / MY_DEFECTS --

    #[test]
    fn test_opp_defects_count() {
        let opp = [Move::Defect, Move::Cooperate, Move::Defect];
        let code = [op::OPP_DEFECTS, op::RETURN];
        // 2 defections → nonzero → defect
        assert_eq!(execute_bytecode(&code, &opp, &[], 3, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_my_defects_count() {
        let my = [Move::Cooperate, Move::Cooperate];
        let code = [op::MY_DEFECTS, op::RETURN];
        // 0 defections → cooperate
        assert_eq!(execute_bytecode(&code, &[], &my, 2, &mut rng()), Move::Cooperate);
    }

    // -- SCORE_LAST --

    #[test]
    fn test_score_last_round_0() {
        let code = [op::SCORE_LAST, op::RETURN];
        // Round 0, no history → default 3 → nonzero → defect
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_score_last_after_betrayal() {
        let code = [op::SCORE_LAST, op::RETURN];
        // I cooperated, opponent defected → payoff = 0
        assert_eq!(
            execute_bytecode(&code, &[Move::Defect], &[Move::Cooperate], 1, &mut rng()),
            Move::Cooperate,
        );
    }

    // -- RAND --

    #[test]
    fn test_rand_range() {
        let code = [op::RAND, op::RETURN];
        // RAND produces 0..99; we just test it runs without panic
        let _ = execute_bytecode(&code, &[], &[], 0, &mut rng());
    }

    // -- Jumps --

    #[test]
    fn test_jmp_fwd() {
        // Jump over DEFECT to COOP
        let code = [op::JMP_FWD, 1, op::DEFECT, op::COOP];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_jmp_fwd_if_taken() {
        // Condition = 1 → jump over COOP to DEFECT
        let code = [op::PUSH, 1, op::JMP_FWD_IF, 1, op::COOP, op::DEFECT];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Defect);
    }

    #[test]
    fn test_jmp_fwd_if_not_taken() {
        // Condition = 0 → fall through to COOP
        let code = [op::PUSH, 0, op::JMP_FWD_IF, 1, op::COOP, op::DEFECT];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    // -- Fail-safe cases --

    #[test]
    fn test_empty_bytecode_cooperates() {
        assert_eq!(execute_bytecode(&[], &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_stack_underflow_cooperates() {
        let code = [op::RETURN]; // pop on empty stack
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_stack_overflow_cooperates() {
        // Push 9 values onto 8-slot stack
        let mut code = Vec::new();
        for _ in 0..9 {
            code.push(op::PUSH);
            code.push(1);
        }
        code.push(op::RETURN);
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_unknown_opcode_cooperates() {
        let code = [0xFF];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_fuel_exhaustion_cooperates() {
        // Infinite loop that never terminates (would run forever without fuel limit)
        // JMP_FWD 0 loops back to itself (pc = pc+2+0 = pc+2, but we need a backward loop)
        // Actually with forward-only jumps we can't create a backward loop.
        // Instead, just run a long chain of NOPs (PUSH+POP would use stack).
        // 129 single-byte NOPs is more than 128 fuel.
        // Use ROUND which is 1-byte and pushes, then POP via SUB or NOT.
        // Simplest: lots of DUP on a value, which uses fuel.
        let mut code = Vec::new();
        code.push(op::PUSH);
        code.push(1);
        for _ in 0..127 {
            // Each DUP + pop pair uses 2 fuel. But DUP needs pop.
            // Just do ROUND + NOT repeatedly, each burns 1 fuel per op.
            code.push(op::NOT);
        }
        code.push(op::RETURN);
        // That's 1 (PUSH) + 127 (NOT) + 1 (RETURN) = 129 ops > 128 fuel
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    #[test]
    fn test_fall_off_end_cooperates() {
        // No terminal instruction
        let code = [op::PUSH, 5];
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }

    // -- Validation --

    #[test]
    fn test_validate_empty() {
        assert_eq!(validate_bytecode(&[]), Err(BytecodeError::Empty));
    }

    #[test]
    fn test_validate_too_long() {
        let code = vec![op::PUSH; MAX_BYTECODE_LEN + 1];
        assert_eq!(validate_bytecode(&code), Err(BytecodeError::TooLong));
    }

    #[test]
    fn test_validate_unknown_opcode() {
        let code = [0xFF, op::COOP];
        assert_eq!(
            validate_bytecode(&code),
            Err(BytecodeError::UnknownOpcode { offset: 0, opcode: 0xFF }),
        );
    }

    #[test]
    fn test_validate_truncated_push() {
        let code = [op::PUSH]; // missing immediate
        assert_eq!(
            validate_bytecode(&code),
            Err(BytecodeError::TruncatedImmediate { offset: 0 }),
        );
    }

    #[test]
    fn test_validate_jump_out_of_bounds() {
        let code = [op::JMP_FWD, 255, op::COOP]; // jump way past end
        assert_eq!(
            validate_bytecode(&code),
            Err(BytecodeError::JumpOutOfBounds { offset: 0 }),
        );
    }

    #[test]
    fn test_validate_no_terminal() {
        let code = [op::PUSH, 5, op::PUSH, 3, op::ADD];
        assert_eq!(validate_bytecode(&code), Err(BytecodeError::NoTerminal));
    }

    #[test]
    fn test_validate_valid_programs() {
        assert!(validate_bytecode(&[op::COOP]).is_ok());
        assert!(validate_bytecode(&[op::DEFECT]).is_ok());
        assert!(validate_bytecode(&[op::OPP_LAST, op::RETURN]).is_ok());
        assert!(validate_bytecode(&[
            op::OPP_DEFECTS, op::PUSH, 0, op::GT, op::JMP_FWD_IF, 1, op::COOP, op::DEFECT,
        ]).is_ok());
    }

    // -- Parity tests: bytecode vs native builtins in full matches --

    use crate::strategy::{Strategy, StrategyBase, PlayerStrategy, execute_strategy};
    use crate::game::run_match;

    /// Run a full match with bytecode vs builtin and assert identical scores.
    fn assert_match_parity(bytecode: &[u8], base: StrategyBase) {
        let seed = [42u8; 32];
        let builtin = PlayerStrategy::Builtin(Strategy::new(base));
        let custom = PlayerStrategy::Custom(bytecode.to_vec());

        // Test both orderings: custom as A and custom as B
        for match_idx in 0..5 {
            let r1 = run_match(&builtin, &builtin, &seed, match_idx, 100);
            let r2 = run_match(&custom, &custom, &seed, match_idx, 100);
            assert_eq!(r1.round_count, r2.round_count, "Round count mismatch for {:?} match {}", base, match_idx);
            assert_eq!(r1.total_score_a, r2.total_score_a, "Score A mismatch for {:?} match {}", base, match_idx);
            assert_eq!(r1.total_score_b, r2.total_score_b, "Score B mismatch for {:?} match {}", base, match_idx);
            for (r_native, r_custom) in r1.rounds.iter().zip(r2.rounds.iter()) {
                assert_eq!(r_native.move_a, r_custom.move_a, "Move A mismatch round {} for {:?}", r_native.round, base);
                assert_eq!(r_native.move_b, r_custom.move_b, "Move B mismatch round {} for {:?}", r_native.round, base);
            }
        }
    }

    #[test]
    fn test_parity_always_cooperate() {
        // AlwaysCooperate = COOP
        assert_match_parity(&[op::COOP], StrategyBase::AlwaysCooperate);
    }

    #[test]
    fn test_parity_always_defect() {
        // AlwaysDefect = DEFECT
        assert_match_parity(&[op::DEFECT], StrategyBase::AlwaysDefect);
    }

    #[test]
    fn test_parity_tit_for_tat() {
        // TitForTat (default params) = OPP_LAST RETURN
        assert_match_parity(&[op::OPP_LAST, op::RETURN], StrategyBase::TitForTat);
    }

    #[test]
    fn test_parity_grim_trigger() {
        // GrimTrigger (default params, noise_tolerance=0) = OPP_DEFECTS PUSH 0 GT JMP_FWD_IF 1 COOP DEFECT
        assert_match_parity(
            &[op::OPP_DEFECTS, op::PUSH, 0, op::GT, op::JMP_FWD_IF, 1, op::COOP, op::DEFECT],
            StrategyBase::GrimTrigger,
        );
    }

    #[test]
    fn test_parity_suspicious_tft() {
        // SuspiciousTitForTat (default params) = round 0 → defect, else copy opponent
        // ROUND PUSH 0 EQ JMP_FWD_IF 1 (skip defect) OPP_LAST RETURN DEFECT
        // Actually: if round == 0 → DEFECT; else OPP_LAST RETURN
        let code = [
            op::ROUND,         // push round number
            op::PUSH, 0,       // push 0
            op::EQ,            // round == 0?
            op::JMP_FWD_IF, 2, // if yes, jump to DEFECT
            op::OPP_LAST,      // else, copy opponent
            op::RETURN,
            op::DEFECT,        // round 0: defect
        ];
        assert_match_parity(&code, StrategyBase::SuspiciousTitForTat);
    }

    #[test]
    fn test_parity_tit_for_two_tats() {
        // TitForTwoTats: defect only if opponent defected last 2 rounds in a row
        // if round < 2 → COOP
        // OPP_N(0) AND OPP_N(1) → RETURN
        let code = [
            op::ROUND,          // push round
            op::PUSH, 2,        // push 2
            op::LT,             // round < 2?
            op::JMP_FWD_IF, 8,  // if yes, jump to COOP
            op::PUSH, 0,        // push 0
            op::OPP_N,          // opponent move 0 rounds ago
            op::PUSH, 1,        // push 1
            op::OPP_N,          // opponent move 1 round ago
            op::AND,            // both defected?
            op::RETURN,         // 1 = defect, 0 = cooperate
            op::COOP,           // cooperate (early rounds)
        ];
        assert_match_parity(&code, StrategyBase::TitForTwoTats);
    }

    #[test]
    fn test_parity_pavlov() {
        // Pavlov: if score_last >= 3 → repeat my_last; else flip
        // Round 0 → COOP (no history)
        // score_last >= 3: repeat; score_last < 3: flip
        // MY_LAST gives 0=Coop, 1=Defect
        // If good outcome: return MY_LAST
        // If bad outcome: return NOT(MY_LAST)
        let code = [
            op::ROUND,          // push round
            op::PUSH, 0,        // push 0
            op::EQ,             // round == 0?
            op::JMP_FWD_IF, 9,  // if yes, jump to COOP
            op::SCORE_LAST,     // push my last score
            op::PUSH, 3,        // push 3
            op::LT,             // score < 3?
            op::JMP_FWD_IF, 3,  // if bad, jump to flip
            op::MY_LAST,        // good outcome: repeat
            op::RETURN,
            op::MY_LAST,        // bad outcome: flip
            op::NOT,
            op::RETURN,
            op::COOP,           // round 0
        ];
        assert_match_parity(&code, StrategyBase::Pavlov);
    }

    /// Test a custom strategy in a match against a builtin.
    #[test]
    fn test_custom_vs_builtin_match() {
        let seed = [42u8; 32];

        // Custom TFT bytecode vs builtin AlwaysDefect
        let tft_bytecode = PlayerStrategy::Custom(vec![op::OPP_LAST, op::RETURN]);
        let always_defect = PlayerStrategy::Builtin(Strategy::new(StrategyBase::AlwaysDefect));

        let result = run_match(&tft_bytecode, &always_defect, &seed, 0, 100);

        // Round 0: TFT cooperates, AD defects → (0, 5)
        assert_eq!(result.rounds[0].move_a, Move::Cooperate);
        assert_eq!(result.rounds[0].move_b, Move::Defect);

        // Round 1+: TFT copies → both defect
        for r in result.rounds.iter().skip(1) {
            assert_eq!(r.move_a, Move::Defect);
            assert_eq!(r.move_b, Move::Defect);
        }
    }

    /// Test round-by-round parity: bytecode TFT vs native TFT
    /// with a specific sequence of opponent moves.
    #[test]
    fn test_round_by_round_parity_tft() {
        let tft_native = Strategy::new(StrategyBase::TitForTat);
        let tft_bytecode = vec![op::OPP_LAST, op::RETURN];

        let opp_sequences: Vec<Vec<Move>> = vec![
            vec![Move::Cooperate; 10],
            vec![Move::Defect; 10],
            vec![Move::Cooperate, Move::Defect, Move::Cooperate, Move::Defect, Move::Cooperate],
        ];

        for opp_moves in &opp_sequences {
            let mut my_history = Vec::new();
            for (round, _) in opp_moves.iter().enumerate() {
                let opp_history = &opp_moves[..round];
                let mut rng1 = SeededRng::new(&[42u8; 32], 0).for_round(round as u8);
                let mut rng2 = SeededRng::new(&[42u8; 32], 0).for_round(round as u8);

                let native_move = execute_strategy(&tft_native, opp_history, &my_history, round as u8, &mut rng1);
                let custom_move = execute_bytecode(&tft_bytecode, opp_history, &my_history, round as u8, &mut rng2);

                assert_eq!(native_move, custom_move, "Mismatch at round {} with opp {:?}", round, opp_history);
                my_history.push(native_move);
            }
        }
    }

    /// Validate that max-length bytecode (64 bytes) works.
    #[test]
    fn test_max_length_bytecode() {
        // 31 PUSH+imm pairs (62 bytes) + COOP + DEFECT = 64 bytes
        let mut code = Vec::new();
        for _ in 0..31 {
            code.push(op::PUSH);
            code.push(0);
        }
        code.push(op::COOP);
        code.push(op::DEFECT);
        assert_eq!(code.len(), MAX_BYTECODE_LEN);
        assert!(validate_bytecode(&code).is_ok());
        // Executes: pushes 31 zeros then hits COOP
        // Stack overflow at push #9 → fail-safe cooperate
        assert_eq!(execute_bytecode(&code, &[], &[], 0, &mut rng()), Move::Cooperate);
    }
}
