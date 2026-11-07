# ZillaOS — фактичний стан та невиконані гейти

**Snapshot:** 2026-11-07, після [PR #19](https://github.com/kabasik007/ZillaOs/pull/19) та [PR #26](https://github.com/kabasik007/ZillaOs/pull/26). Цей документ є датованим зрізом, а не динамічною телеметрією. Перед новою роботою перевір актуальний HEAD `main`, CI та [I-M2 #25](https://github.com/kabasik007/ZillaOs/issues/25).

## Точні терміни доказів

- **Source:** функція або модель присутня в коді.
- **Host-verified:** тест справді виконувався на хості або в CI.
- **Build-verified:** компілюється на `x86_64-unknown-none`, **але** це не доводить, що функція працює в гостьовій ОС.
- **QEMU-verified:** конкретний сценарій реально виконувався у QEMU/OVMF з серійними/assertion логами.
- **VMware-observed:** користувач раніше показав працездатний GUI у VMware; CI QEMU не заміняє новий GUI тест у VMware.
- **Hardware-verified:** на фізичному цільовому пристрої зафіксовано поведінку. **Для AMD GPU / справжнього SMP таких доказів немає**.

## Реєстр можливостей

| Домен | Що є зараз | Докази | Що НЕ можна заявляти |
| --- | --- | --- | --- |
| UEFI boot / serial | Limine, `src/main.rs`, інструменти ISO і QEMU | QEMU boot/smoke CI; попередній VMware GUI | production installer або hardening |
| Desktop / WindowManager | software-rendered вбудовані views, Start, Dock, Files, Settings, Notepad | host regression, QEMU desktop; попереднє підтвердження користувачем у VMware | окремі desktop-процеси, GPU compositor |
| RAMFS / storage | обмежений volatile RAMFS, GPT parser та безпечні disposable RAW/ESP експерименти | host/installer tests, QEMU окремих сценаріїв | стале збереження користувацьких файлів на фізичний диск |
| Process/Scheduler | PID/TID, CPU0 scheduler/lifecycle/stack models | host tests; opt-in QEMU Ring3 threads `ABABAB` і preempt `ABABABAB` | production multi-process/SMP scheduler |
| User CPU boundary | opt-in окремі експериментальні native Ring3 fixtures з fault handling | QEMU supervisor access denied, guard-page `#PF`, clean return | усі застосунки Desktop захищені Ring3 |
| IPC/capabilities | `src/kernel/ipc/**`, `capabilities/**`, generation/rights/queue/cleanup models | Rust host negative tests | production syscalls, аудит усіх прав |
| ServiceManager | typed ServiceSupervisor, dependency graph, restart/backoff і model-only launch intents | host/no_std tests | працюючий `zillad`, user-space service |
| Platform / PCI | `BootPci`, bounded read-only single inventory, DeviceRegistry, Storage/VMware/AMD views | host tests, QEMU PCI inventory smoke | багатосегментний PCIe/ECAM, DMA/IRQ driver |
| AMD Radeon | PCI identity/ASIC hints та inert diagnostic IP contracts | host tests | hardware init, firmware, display output чи acceleration |
| VMware SVGA / Display | read-only discovery, FIFO math, surface/output/modeset contracts | host + QEMU software fallback | реальний SVGA FIFO command submission, native modeset |
| PE64 | inspector/parsing | host tests | Windows EXE/Win32 execution, DLL imports |

## Зафіксовані CI докази I-M1

**Той самий код PR #26:** [cross-workstream gate #42336227902](https://github.com/kabasik007/ZillaOs/actions/runs/42336227902), усі чотири jobs PASS:
- 385 `cargo test --lib`, 20 ServiceManager host, 14 ServiceSupervisor host, 34 canonical-PID bridge, 127 Python regression — **580 тестів PASS**;
- `cargo +1.90.0 fmt --all -- --check` — **PASS** після окремого Rustfmt PR;
- чотири release/bare-metal конфігурації — **PASS**;
- QEMU/OVMF Ring3 smoke, user guard fault і supervisor access denial — **PASS**.

Окремі CI на тому ж PR head: [Platform/QEMU](https://github.com/kabasik007/ZillaOs/actions/runs/42336227877), [Process/QEMU](https://github.com/kabasik007/ZillaOs/actions/runs/42336227910), [IPC](https://github.com/kabasik007/ZillaOs/actions/runs/42336228450), [Services](https://github.com/kabasik007/ZillaOs/actions/runs/42336227948), [Graphics](https://github.com/kabasik007/ZillaOs/actions/runs/42336228009), [AMD](https://github.com/kabasik007/ZillaOs/actions/runs/42336227938), [PCI](https://github.com/kabasik007/ZillaOs/actions/runs/42336227923), [Windows Builder](https://github.com/kabasik007/ZillaOs/actions/runs/42336227916) — PASS. Це перевірка визначених workflow, не загальна сертифікація ОС.

## Що блокує реальну багатозадачну ОС

1. [Issue #21](https://github.com/kabasik007/ZillaOs/issues/21) — production kernel-owned identity, handle lifetime і IPC teardown у реальній syscall boundary.
2. [Issue #22](https://github.com/kabasik007/ZillaOs/issues/22) — довірений зв'язок Services/Device/Display lifecycle, authorization, revocation, реальний backend для user-space services.
3. SMP, драйвери та GPU: реальна багатоядерність, фізичні MMIO/IRQ/DMA, керування ресурсами й апаратні графічні тести **ще не активовані**.
4. Зовнішні ELF/PE/Android runtime і постійний дисковий стек ще не придатні для звичайного використання.

**Важливе розділення:** повний CI є гейтом якості *конкретного коміту*; статус окремої функції визначається її власними runtime-тестами. Плани і моделі — не реалізована поведінка.
