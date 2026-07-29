# Atmospeak product strategy

Written 2026-07-29. This document sets the frame that later product and
engineering decisions are measured against. It supersedes the framing — not the
task list — of [`paritygoal.md`](../paritygoal.md), which chases feature parity
with Wispr Flow. Parity is a milestone, not a strategy.

## 1. The frame: displacement, not parity

Atmospeak 0.5.3 already works. It runs `whisper.cpp` locally, streams partials
with VAD, corrects speech deterministically with Backtrack, optionally polishes
on-device with a bundled `llama-server`, and pastes in about 1.9 s from key
release. It requires no account and sends no microphone audio anywhere.

What it does not have is a business model, a reason for a compliance officer to
approve it, or a build that runs anywhere but Windows.

Chasing parity means arriving second at somebody else's product. The goal is to
be the product a specific set of users switches *to* for reasons the incumbent
cannot engineer away.

## 2. Positioning

> Wispr Flow rents you your own voice. Atmospeak sells it to you once.

Three claims a cloud subscription competitor cannot match, because their
architecture and their business model both forbid it:

| Claim | Why it is structurally hard to match |
| --- | --- |
| **Works with the network cable unplugged.** | Cloud ASR plus cloud LLM formatting is the whole pipeline. |
| **No account, ever.** Nothing to sign into, nothing to leak, nothing to cancel. | Subscription billing requires identity. |
| **Pay once. It is yours.** No renewal, no seat audit, no price increase. | Recurring revenue is the valuation story. |

### What we do not claim

Atmospeak does not claim to beat a well-funded cloud stack on raw accuracy for
hard audio. It claims **predictability**: the same latency on a plane as at a
desk, the same behaviour in year three as on day one, and no feature that
disappears when a subscription lapses.

Comparative claims about competitors belong in
[`WISPR_FLOW_COMPETITIVE_AUDIT.md`](WISPR_FLOW_COMPETITIVE_AUDIT.md), sourced and
dated. Do not put unsourced competitor claims in marketing copy.

## 3. Four improvement tracks

Ordered by leverage per week of work, not by size.

### Track A — Privacy and compliance as a shippable artifact

Atmospeak is already local. The work is turning "local" from something we assert
into something a buyer can verify.

- **Attested airplane mode.** A hard mode in which the dictation path refuses
  outbound sockets, surfaced as a persistent UI state rather than a buried
  setting. Builds on the CSP posture already in `src-tauri/tauri.conf.json`,
  which whitelists only `github.com` and `api.github.com`.
- **Network ledger.** A user-visible, append-only record of every outbound
  connection the app has made: model downloads, update checks, nothing else.
  The local `runtime_events` table is the natural substrate.
- **Retention and redaction.** `transcript_retention_days` already exists in
  `src-tauri/src/db/mod.rs`. Extend to per-application retention rules and PII
  redaction applied before anything reaches disk.
- **Compliance pack.** Exportable audit log, data-flow diagram, a DPA-ready
  written statement, and an offline deployment guide.

This track sells to legal, medical, financial, and public-sector buyers who
cannot approve a tool that ships audio to a third party at all.

### Track B — Agentic and developer workflows

People dictating into coding agents, editors, and terminals are a large share of
this market and the place a cloud tool is weakest.

- **Voice-to-action macros** (`D-02`). Dictation that dispatches a command
  instead of pasting text. This requires an explicit, user-reviewable allowlist;
  never construct a shell command directly from ASR output.
- **MCP server.** Expose dictation as a Model Context Protocol tool so coding
  agents can request and consume speech. This turns a dictation app into agent
  infrastructure.
- **Editor symbol awareness** (`T1-09`, `T1-10`). Feed the focused editor's
  symbols to Whisper as vocabulary so identifiers transcribe correctly. Reading
  the accessibility tree locally achieves this without screen contents ever
  leaving the machine.
- **Per-application cleanup profiles** (`D-03`). Raw symbols in an editor,
  casual tone in chat, formal in mail.

### Track C — Quality and latency

Wispr's publicly stated engineering target is a complete ASR and LLM-formatted
result within 700 ms of speech ending. Atmospeak's measured release-to-paste
budget is about 1.9 s. That gap has to close.

- **Finish the Vulkan sidecar.** `src-asr-host` already has the `vulkan`
  feature, but it failed to build in the last logged run
  (`tests/manual/production-run-log.md`). GPU inference is the largest single
  latency win available.
- **Speculative paste.** Commit the stable high-confidence prefix while the tail
  is still decoding.
- **Correction learning** (`T1-12`). Mine the raw-versus-final diff to propose
  dictionary entries, with a visible, per-application, deletable record of what
  was learned.
- **Published benchmarks.** Measured p50 and p95 latency per model per hardware
  tier. A competitor publishes a target; we can publish evidence.

### Track D — Platform reach

Windows-only caps the addressable market hard. `docs/MACOS_FOLLOWUP.md` exists
but is unstarted. macOS is the highest revenue-impact engineering project on
this list and also the most expensive: CGEvent injection, AXUIElement, and
notarisation each carry real cost.

Sequence macOS *after* the entitlement layer exists, so the port lands in a
product that can already take money.

**Also blocking revenue on Windows today:** installers are unsigned, so
SmartScreen warns on every download. `.env.example` records this as an
intentional prototype decision. A trusted certificate or an Azure Trusted
Signing profile is a hard prerequisite for charging money. It is a purchasing
decision, not an engineering one.

## 4. Revenue architecture

Atmospeak is **open core**. The core is not a limited demo of the paid product;
it is the reason anyone trusts the paid product.

### Free forever, MIT, unlimited, no account

Hotkey capture and chord recording, every local Whisper model and the model
downloader, streaming ASR and the Vulkan/CPU fallback ladder, cleanup,
Backtrack, injection, history with search and export, dictionary, snippets,
overlay customisation, and on-device LLM polish.

**Everything shipped through 0.5.3 stays free.** This is a standing commitment,
not a launch promotion. Retroactively paywalling a shipped feature would destroy
the trust the entire product depends on, and the MIT grant on already-published
code cannot be withdrawn from anyone who already has it.

### Pro — one-time perpetual licence

Includes one year of updates and continues working offline, forever, for every
build released inside that window. Pro consists **only of capability that does
not exist yet**:

- Compliance pack: attested airplane mode, network ledger, audit export
- Voice-to-action macros and the MCP server
- Editor symbol awareness and per-application profiles
- Encrypted cross-device sync of dictionary, snippets, and settings

This is the honest form of open core: Pro is paid because it is *new*, not
because something was taken away.

### Team

Shared dictionary and snippets, offline seat provisioning, an on-premises
deployment guide, and the compliance artifacts from Track A. Aimed at buyers who
currently have no option but a "contact sales" enterprise tier.

### Why a perpetual licence

A subscription competitor's revenue depends on renewal. Every user who switches
to a perpetual licence is removed from that base permanently, which costs a
subscription business more than the one-time payment earns us. Subscription
fatigue is also the most consistently cited complaint in this category.

**On price.** Comparable perpetual licences in this market sit well above where
we intend to launch — Superwhisper's lifetime tier is $249.99 and Voibe's is
$149. A launch price near $59 is therefore deliberately under market, and should
be framed as an early-adopter tier with an explicit commitment that the price
rises after a stated number of licences, and never rises for anyone who already
bought. The perpetual *format* is what breaks the incumbent's grip; the number
has room to move.

### Explicitly not our model

- No usage metering on local dictation. Local inference costs us nothing; metering
  it would be rent extraction and would contradict the positioning.
- No telemetry as a revenue input.
- No feature that stops working because a licence lapsed. Lapsing ends *updates*,
  not the software.

## 5. Distribution

The strategy needs volume to work.

1. An offline and privacy comparison page, to own the "local dictation" and
   "Wispr Flow alternative" search intent.
2. Published latency benchmarks, since evidence is the differentiator against a
   competitor that publishes targets.
3. The network ledger as a screenshot. It is the single most persuasive artifact
   we can produce for a privacy-motivated audience.
4. Ship the MCP server where agent-tooling users already are.

## 6. Non-goals

- Cloud ASR as the default path. An optional bring-your-own-key escape hatch is
  acceptable; a default that sends audio off-machine is not.
- Anti-tamper or DRM measures that compromise the offline guarantee. See
  `src-tauri/src/services/license.rs` — offline verification is defeatable and
  that is an accepted trade.
- Accounts. Not for licensing, not for sync, not for support.
