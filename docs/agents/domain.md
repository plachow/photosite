# Domain docs

This is a single-context repository. Engineering skills should consume its
domain documentation as follows.

## Before exploring

- Read `CONTEXT.md` at the repository root when it exists.
- Read ADRs under `docs/adr/` that touch the area being changed.
- If these files do not exist, proceed silently. The producer workflow creates
  them lazily when domain terms or architectural decisions are resolved.

## Use the project vocabulary

When output names a domain concept, use the term defined in `CONTEXT.md`. Avoid
synonyms that the glossary explicitly rejects. If a needed concept is absent,
reconsider whether new language is necessary and note a genuine gap for the
domain-documentation workflow.

## Flag ADR conflicts

If a proposed change contradicts an existing ADR, surface that conflict
explicitly rather than silently overriding the decision.
