# ZillaOS × Spec-First Bootstrap — аналіз і рішення

**Status:** methodology adapted, **bootstrap engine not installed or run against ZillaOS**.  
**Date:** 2026-11-07 · **Owner:** Chief Architect / documentation.

## Джерела

- [Наша розширена версія](https://github.com/kabasik007/spec-first-bootstrap) — Universal AI Development Bootstrap (local-first, autonomous repository discovery, architecture/standards/roadmap, readiness questions).
- [Оригінал Potapenko](https://github.com/potapenko/spec-first-bootstrap) — spec-first setup and governance, окремий optional browser QA.
- Поточні ZillaOS [AGENTS](../AGENTS.md), [I-M2](NEXT_WAVE_I_M2.md), [Mission Control #25](https://github.com/kabasik007/ZillaOs/issues/25).

**Важливо:** ці два repositories мають відмінну структуру та зміст. Для ZillaOS взято принципи з нашого fork, а не копію всього upstream installer чи його configuration. Усе нижче — **адаптація**, не твердження про автоматичний запуск bootstrap.

## Що беремо з методології

| Принцип Bootstrap | Як застосовано до ZillaOS |
| --- | --- |
| Discover first, ask second | Source/CI/target environment перевіряються перед питаннями про архітектуру |
| Separate truths | `STATUS` = evidence, `specs` = behavior, `ARCHITECTURE_CURRENT` = boundaries, `DECISIONS` = rationale, `NEXT_WAVE` = plan |
| Spec-first | Для kernel ABI, IPC message shape, process exit, device/driver interfaces, persistence, GUI behavior — явні інваріанти й failure policy |
| Brownfield preservation | Не замінюємо без доказу вже створені модулі, не міняємо Cargo features, не переносимо всі старі docs одним rewrite |
| Readiness gate | Функція має blockers і DoD; `host check PASS` ≠ `physical hardware ready` |
| Retrieval-friendly rules | Короткий `AGENTS.md` + навігатор `docs/README.md` + доменні контракти |
| Explicit ownership | 6 незалежних PR/гілок, тільки Chief Architect інтегрує shared файли |
| Risk-aware verification | Negative/fault QEMU, host mocks, native/VMware/AMD lab evidence описані окремо |

## Чого **не переносимо** автоматично

- Не копіюємо стандартні browser QA Playwright правила: ZillaOS — UEFI/Rust kernel, а не вебсайт.
- Не запускаємо `bootstrap.py onboard` без окремого review: він може перегенерувати `AGENTS.md`, `ROADMAP.md`, `docs/ARCHITECTURE.md` та `.ai/` і перезаписати реальні контракти шести агентів.
- Не створюємо нову мікросервісну архітектуру лише тому, що є папка `services/`. У нас поки Rust kernel + **model-only** ServiceManager.
- Не додаємо `.ai` машинний індекс із вигаданими статусами. `ready_for_implementation` має відображати реальні blockers за конкретним scope.
- Не дублюємо діючі документи в новій ієрархії й не переносимо чужі файли без перевірки inbound посилань.

## Безпечний майбутній dry-run справжнього engine

Для окремої локальної **копії** ZillaOS, не для `main` і не для робочої директорії зі змінами:

```bash
# Приклад після ручного клонування/огляду bootstrap:
python3 /path/to/spec-first-bootstrap/bootstrap.py detect /path/to/ZillaOs-copy \
  --intent "document Rust no_std x86-64 kernel and six-agent workstreams"
```

Після read-only `detect`: перевір виявлені Rust/toolchain target, entry points, dependencies, modules і blocking questions. `onboard/init` **пише файли**, тому допускається лише в окремому review-PR із diff/rollback. Python 3.9+ потрібен bootstrap engine на **хості**, не ядру ZillaOS.

## Наш spec lifecycle

1. **Discovery**: посилання на конкретні source files/CI і датовану перевірку.
2. **Contract**: `docs/specs/features/<slug>.md` (observable result, invariants, failure cases, non-goals).
3. **Review**: свіжий агент перевіряє контракти та вихідні твердження, визначає BLOCKED/READY/NEEDS CUTS — не вдає, що незавершені hardware dependencies вирішені.
4. **Implement**: код у owner PR, тести з негативними сценаріями, без прихованих shared edits.
5. **Evidence + decision log**: CI URL на SHA, окреме позначення MODEL/QEMU/HARDWARE і ADR при зміні важливої політики.
6. **Handoff**: закриваємо/переносимо blocker в Issue #25, оновлюємо `STATUS` лише після підтвердження.

## З чого почати

Приклади власних контрактів:
- [Single boot PCI inventory](specs/features/single-pci-inventory.md) — source/CI-verified I-M1 read-only behavior.
- [Trusted process exit cleanup](specs/features/trusted-process-exit.md) — I-M2 proposal/blocked on production boundary.
- [Шаблон](specs/templates/feature-spec.md) — для наступних агентських задач.

Спочатку перевір, що методика **допомагає** агентам і не сповільнює дрібні технічні правки. Поріг: spec потрібен, коли міняється behavior, ABI, rights, process/device ownership або user-visible workflow.
