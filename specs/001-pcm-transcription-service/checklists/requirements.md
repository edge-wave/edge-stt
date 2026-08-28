# Specification Quality Checklist: PCM Transcription Service

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-08-28
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

- **Iteration 2 (2026-08-28)**: all items pass. Both clarifications were answered — progressive partial transcripts are in the first release on both backends (User Story 3, FR-012–FR-016), and the transcription server is delivered by this project (User Story 4, FR-017–FR-024). Functional requirements were renumbered when the server group was added; no downstream artifact referenced the old IDs.
- **Deliberate, reviewed and accepted**: three concrete technologies are named, all confined to the Assumptions section and none binding a requirement.
  - The Whisper model family — the user set "use a well-known open model" as the first goal. FR-025 states the requirement neutrally ("publicly available open-weight model").
  - WebSocket — named only as prior art from the echo-vinci prototype that informs planning. FR-014 states the requirement neutrally ("through the same caller-facing mechanism"); the transport is chosen in `/speckit-plan`.
  - Tailscale — named only to describe the trust environment that justifies FR-020's shared-credential model rather than per-user accounts.
- **Amendment, 2026-08-28 (during `/speckit-plan`)**: SC-002 and SC-005 originally promised
  absolute speeds on the reference edge device — real-time transcription, and a first partial
  within one second. Research showed those depend on model size, board, and language, none of
  which this project chooses. Promising them would have taken a decision away from the
  integrator, who is the only one able to make it. Both were rewritten to measure what edge-stt
  itself contributes — overhead over the model (SC-002), and delivering a partial the moment the
  decoder produces it (SC-005) — and the trade became an API surface instead: FR-027 gives the
  integrator model size, threads, and accelerator; FR-028 forbids refusing a combination for
  being slow; SC-012 publishes measurements per size per machine; SC-013 lets an integrator
  reproduce them on their own board; FR-036 reports the real-time factor at runtime. Functional
  requirements after FR-027 shifted by one; total is now 36.
- **Still to be measured, not blocking**: SC-008 and SC-009 quote figures (8 concurrent clients,
  8 hours, 1,000 utterances) that are the server's own scheduling behaviour and are testable as
  written. SC-012's table has no numbers in it until the benchmark command has run on both
  reference machines, which plan.md schedules as an early task.
