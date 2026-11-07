# ZillaOS — матриця перевірок і доказів

**As of:** 2026-11-07 · **Status:** verified snapshot of I-M1 code at PR #26. Не підміняє live CI наступних комітів.

## Загальні гейти

| Gate | Що перевіряє | Доказ / очікувана команда | Межа |
| --- | --- | --- | --- |
| Formatting | Консистентність Rust 1.90 | `cargo +1.90.0 fmt --all -- --check`; [CI #42336227902](https://github.com/kabasik007/ZillaOs/actions/runs/42336227902) PASS | Не доводить безпеку |
| Host Rust | no_std-compatible models / negative unit tests | `cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline`; **385 PASS** | Host ≠ kernel runtime |
| ServiceManager tests | supervisor/state machine; canonical PID bridge | 20 + 14 + 34 PASS у інтеграційному CI | Service model ≠ launched daemon |
| Python regressions | scripts, Windows builder, installer/file safety | `python3 -m unittest discover -s tests -v`; **127 PASS** | Host tests ≠ VM |
| Bare-metal target | `x86_64-unknown-none` default/`vm-demo`/`native-ring3-threads`/`native-ring3-preempt` | **4 builds PASS** | Compile ≠ boot |
| QEMU Native Ring3 | opt-in fixture boot and clean return | [CI #42336227902](https://github.com/kabasik007/ZillaOs/actions/runs/42336227902) | Не universal process launch |
| QEMU fault negatives | guard-page fault, supervisor access denied | serial `ZILLAOS_NATIVE_USER_GUARD_FAULT_OK`, `ZILLAOS_NATIVE_USER_KERNEL_ACCESS_DENIED` | Не production syscall security |
| QEMU PCI / Desktop | bounded boot inventory і software Desktop | [Platform #42336227877](https://github.com/kabasik007/ZillaOs/actions/runs/42336227877) | Не physical GPU / VMware SVGA |
| QEMU scheduler | threads `ABABAB`, preemption `ABABABAB` | [Process #42336227910](https://github.com/kabasik007/ZillaOs/actions/runs/42336227910) | Не SMP |
| Windows builder | parser/entrypoint/staging test | [#42336227916](https://github.com/kabasik007/ZillaOs/actions/runs/42336227916) PASS | Local VMware after doc rewrite: NOT RUN |

## Фізичний/продакшн hardware gate

**NOT RUN / NOT IMPLEMENTED:** фізичний AMD GFX/DCN/SMU/PSP, DMA/IRQ, real PCIe ECAM multi-segment, active SVGA FIFO/native modeset, true SMP AP start, protected launch/IPC-backed `zillad`, production disk writes. Не можна виводити "pass" із зеленого Rust CI.

## Доказ для кожної нової функції

При створенні PR запиши:
- source **commit SHA**, точні features/target і test command;
- expected markers та результат **PASS / FAIL / NOT RUN**;
- CI URL або artifact + середовище (host Linux, QEMU, VMware або фізичне залізо);
- negative behavior (stale handle, malformed config, out-of-bounds, crash, timeout);
- реальні missing gates та власника блокера.

Якщо runtime не тестували, пишіть `NOT RUN`, а не `PASS`. Див. [STATUS](../STATUS.md), [VALIDATION](../VALIDATION.md), [development](../DEVELOPMENT.md).
