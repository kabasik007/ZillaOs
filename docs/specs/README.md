# ZillaOS — специфікації поведінки (spec-first)

**Status:** active contract library starting I-M2 · **2026-11-07**.  
**Sources:** [our autonomous bootstrap](https://github.com/kabasik007/spec-first-bootstrap), [Potapenko original](https://github.com/potapenko/spec-first-bootstrap); adapted to Rust `no_std`, OS interfaces and security-boundary verification.

## Чому тут є specs

`AGENTS.md` визначає **як працює агент**, `ARCHITECTURE_CURRENT.md` — **як розділено систему**, `docs/specs/features/` — **якою має бути поведінка**, `tests/` і CI — **як це перевірити**. Не намагайся зафіксувати усі 4 типи правди в одному README.

## Коли обов'язковий новий/оновлений spec

- public/kernel ABI, IPC envelope, version і права;
- process/task lifecycle, syscall isolation, exceptions, capability revoke;
- device resource ownership, PCI scan policy, GPU surfaces/outputs;
- persistent storage, installer, destructive actions;
- user-visible Desktop/Setup/File behavior;
- будь-який multi-agent interface, який може конфліктувати.

Для простих форматувань, опечаток, коментарів та суто механічних no-behavior змін окремий spec не потрібен.

## Структура

```text
docs/specs/
  README.md
  templates/
    feature-spec.md
  features/
    single-pci-inventory.md     verified read-only behavior
    trusted-process-exit.md     proposed next phase, not production
```

## Status і правила evidence

Кожен spec має поля **Status** (`Draft / Reviewed / Implemented-in-source / Verified-in-QEMU / Blocked`), **Owner**, **Source evidence**, **Non-goals** та **Verification gates**. Не називай специфікацію `Verified` лише тому, що її написано або є host test.

1. Вивчи теперішній код і CI, зафіксуй реальні source paths + date/SHA.
2. Створи spec за [шаблоном](templates/feature-spec.md); не вигадуй вимог, яких немає в repo/запиті.
3. Проведи fresh review: розділи **FACT**, **PROPOSAL**, **UNKNOWN** і необхідні **blocking** питання.
4. Якщо spec впливає на shared контракт — Chief Architect погоджує його перед паралельним кодом.
5. Реалізуй у власному PR, зафіксуй negative tests і CI.
6. Онови статус відповідно до фактичних тестів, пов'яжи Issue і PR.

## Поточні приклади

- [Single PCI inventory](features/single-pci-inventory.md) — canonical read-only contract та межі.
- [Trusted process exit cleanup](features/trusted-process-exit.md) — наступний I-M2 security gate; не вважати active production syscall.

Окреме QA/verification: [матриця доказів](../verification/EVIDENCE_MATRIX.md). Архітектурні trade-offs: [DECISIONS.md](../DECISIONS.md).
