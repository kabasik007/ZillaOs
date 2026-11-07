# SPEC: <коротка назва функції>

**ID:** <I-M2-...>  
**Status:** Draft | Reviewed | Implemented-in-source | Verified-in-QEMU | Blocked  
**Owner:** <один agent stream>  
**As of:** YYYY-MM-DD  
**Source facts:** <links to exact paths + evidence/commit>  
**Dependencies:** <issue / other spec / shared contract>

## Goal (WHAT)

Який конкретний результат/поведінка має стати можливим? Один або два короткі абзаци.

## Scope / non-goals

- Included: …
- Excluded: … (особливо непротестовані апаратні або runtime можливості)

## Observable behavior / contract

1. WHEN … THEN …
2. GIVEN … WHEN … THEN …
3. Для невдалого запиту повернути визначений fail-closed результат; не вигадувати успіх.

## State / identity / owner

- Canonical owner: …
- Unique IDs, generation, version, rights: …
- Caller trust boundary (user payload != kernel-owned identity): …
- Memory/resource limits: …

## Invariants

- …
- …
- No unsafe hardware writes / no PID spoofing / no unbounded queue etc. when applicable.

## Failure / adversarial cases

| Input/condition | Expected behavior |
| --- | --- |
| Invalid / stale handle | Reject, no partial mutation |
| Timeout/crash/overflow | Bounded failure, cleanup/rollback |
| Incompatible peer | Fail closed, no silent fallback to unsafe state |

## Verification and evidence

| Gate | Exact test/command or CI | Status |
| --- | --- | --- |
| Host | … | NOT RUN |
| Bare-metal compile | … | NOT RUN |
| QEMU runtime | … | NOT RUN |
| Physical VMware / AMD | … | NOT RUN |

## Readiness / open questions

- **Blocking:** …
- **Advisory:** …
- Do not claim implemented until its specific gate passes.

## Review / delivery

- Reviewed by (independent): …
- PR/commit: …
- Trade-off/ADR link: …
