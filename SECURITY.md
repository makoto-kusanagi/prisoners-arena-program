# Security Policy

## Reporting a Security Vulnerability

**DO NOT CREATE A GITHUB ISSUE** to report a security vulnerability.

Instead, please use GitHub's private vulnerability reporting to submit your
report:

**[Report a Vulnerability](https://github.com/makoto-kusanagi/prisoners-arena-program/security/advisories/new)**

Please include:

- A clear description of the vulnerability and its potential impact
- Step-by-step reproduction instructions
- A working proof-of-concept exploit

Speculative reports without a proof-of-concept will be closed without further
consideration.

Expect a response within 72 hours. If you do not receive a timely response,
send an email to **<security@prisoners.arena>** with the advisory URL only. Do
**not** include exploit details in the email.

## Scope

The following components are in scope:

| Component | Path |
| --- | --- |
| Solana program | `programs/prisoners-arena/` |
| Match logic crate | `crates/match-logic/` |

### Out of scope

- The web frontend
- Bugs in upstream dependencies (Anchor, Solana SDK) — report those upstream
- Social engineering or phishing attacks
- Denial-of-service attacks against RPC nodes or web infrastructure
- Automated scanner output without a working proof-of-concept

## Severity Classification

| Severity | Examples |
| --- | --- |
| Critical | Unauthorized fund withdrawal, bypassing commit-reveal to read hidden strategies, manipulating tournament outcomes or payout distribution |
| High | Griefing attacks that lock funds, bypassing entry stake requirements, state corruption that halts tournaments |
| Medium | Incorrect score calculation, rent-exemption edge cases, minor economic exploits |
| Low | Informational findings, gas optimizations, non-exploitable edge cases |

## Bug Bounty

This project does not currently operate a formal bug bounty program. However,
we value responsible disclosure and will consider rewards on a case-by-case
basis at the project's discretion, commensurate with the severity and impact
of the reported vulnerability.

## Incident Response

1. **Accept** — Acknowledge the report and create a draft security advisory
2. **Triage** — Assess severity and determine affected components
3. **Fix** — Develop and verify a patch in a private fork
4. **Deploy** — Ship the patched program to mainnet
5. **Disclose** — Publish the advisory and notify affected users
