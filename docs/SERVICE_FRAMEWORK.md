# ZillaOS — Service Framework / ServiceManager (`zillad`)

> **Проєктний контракт, не опис готового демона.** Зріз `main`: 2026-11-07. Спершу host-testable data model, після Ring 3 + IPC — user-space `zillad`. Читати [OS_SUBSYSTEMS.md](OS_SUBSYSTEMS.md), [PROCESSES.md](PROCESSES.md), [SECURITY_ARCHITECTURE.md](SECURITY_ARCHITECTURE.md).

## 1. Навіщо служби

Служба — керований компонент із задекларованими залежностями, правами, життєвим циклом і політикою відновлення. **Rust module under `src/services` != user-space service**. Сьогодні `ApplicationManager`, `ProcessManager`, `TaskSnapshot` є звичайними linked модулями, а не PID із власним адресним простором.

Ціль: один `ServiceManager` як supervisor, який запускає *лише дозволені* служби через **єдиний ProcessManager**, а не запускає код чи виконує «магічний рестарт» без контролю ресурсів. У ранньому kernel-demo — лише safe manifest parser, resolver і state machine.

## 2. Базові служби майбутньої ОС

| Service ID (пропозиція) | Призначення | Критичні залежності | Фаза |
| --- | --- | --- | --- |
| `zillad` | dependency graph, launch/stop/restart, health | ProcessManager/IPC/caps | після Ring 3 |
| `deviced` | каталог пристроїв, driver binding, hotplug | kernel DeviceRegistry | model now, daemon later |
| `displayd` | compositor/display orchestration, surfaces | DisplayManager + device handles | model now, daemon later |
| `inputd` | routing events from HID/PS2 | input event handles | later |
| `logd` | bounded event consumption, rotation | kernel EventLog | later |
| `securityd` | policy administration / UI approval | kernel cap enforcement | later |
| `storaged` | block/mount services | trusted block/VFS primitives | later |
| `netd` | network config, protocols | NIC driver/DMA | later |
| `audiod` | mixing, sound-device policy | audio backend | later |
| `powerd` | power/thermal/suspend policy | ACPI, timers, device states | later |
| `updated` | verified package/update operations | storage, signer, rollback | much later |

**Security boundary**: `securityd` не може «дозволити собі все»; остаточний authorization виконує kernel/capability table при кожній операції. `deviced` не надає сирий I/O будь-якому клієнту. `displayd` не здійснює PCI MMIO з UI.

## 3. Service manifest v0 (план)

У v0 визначити компактну статичну Rust-модель для built-in allowlist, без довіри до зовнішніх файлів. JSON/TOML — **приклад людиночитного опису**, а не реалізований parser/ABI:

```toml
id = "displayd"
version = 1
exec = "/System/Services/displayd" # майбутній ELF, сьогодні не запускається
start = "on-demand"                 # on-boot | on-demand | manual
restart = "on-failure"              # never | on-failure
restart_limit = 3
requires = ["deviced"]
after = ["logd"]
capabilities = ["gpu.surface", "device.display", "ipc.connect:deviced"]
max_memory_bytes = 33554432
max_threads = 4
```

Правила:
- `id` унікальний, ASCII bounded; `version` і manifest schema version перевіряються окремо.
- `requires` — hard dependency; `after` — лише ordering, без guarantee availability.
- Cycle, missing required dependency, duplicate ID, over-capacity, invalid capability та invalid path -> **reject before launch**.
- `exec` має бути allowlisted/signed *before* foreign execution; не запускати довільний шлях через manifest.
- `capabilities` — заявлені бажані права, а не автоматичні grants; рішення політики та issuer handle належать kernel.
- Quotas перевіряє kernel, а не `zillad` UI.

## 4. Lifecycle та API (пропозиція)

```text
Discovered -> Validated -> WaitingDeps -> Starting -> Running
                  |             |             |
               Rejected       Blocked      Failed -> Backoff -> Starting
                                               |
                                    Stopping -> Stopped
```

Мінімальні pure types:

```rust
// Sketch, NOT implemented in repository.
enum ServiceState {
    Discovered, Validated, WaitingDeps, Starting, Running,
    Backoff, Stopping, Stopped, Failed, Rejected,
}
enum RestartPolicy { Never, OnFailure }
struct ServiceId(u32);
struct ServiceRuntime {
    id: ServiceId,
    pid: Option<u64>, // only ProcessManager assigns real PID
    state: ServiceState,
    restarts: u16,
}
```

Пізніше API:
- `validate(manifests) -> Result<ServiceGraph, Error>`
- `start(id)`, `stop(id, mode)`, `restart(id)`
- `snapshot() -> bounded read-only service list`
- `on_process_exit(pid, reason)`; не покладатися на GUI callbacks
- `on_dependency_failed(id)` -> stop/mark degraded downstream відповідно до політики
- `on_device_event(DeviceId, Event)` -> повідомляти only-authorized subscribers

Реальні timeout/backoff повинні користуватися kernel monotonic timer; до появи таймера — тестовий clock, без невизначених busy-loops.

## 5. Boot stages

| Stage | Що дозволено | Що не можна |
| --- | --- | --- |
| `EARLY_KERNEL` | serial, memory, IDT, GOP, read-only PCI | spawn services, arbitrary firmware writes |
| `KERNEL_READY` | hardware registry, resource claims, device probe, fallback display | виконувати сторонні драйвери як «служби» без isolation |
| `USER_READY` | kernel Ring 3, syscall + IPC, verified native image | запуск із невідомим signer і без cap table |
| `SERVICES_READY` | `zillad`, boot critical services, health protocol | приховані/необліковані PID |
| `DESKTOP_READY` | UI, Settings, Task Manager, apps | дубльовані process tables |

На ранньому етапі можна реалізувати **ServiceManagerModel без запуску** і логувати `ZILLAOS_SERVICES_MODEL_READY` лише після фактичного тестового підключення. Не вводити такого маркера завчасно.

## 6. Failures / Recovery / Security

- Crash service -> kernel process exit event -> supervisor; restart bounded, exponential backoff, circuit-breaker. Ніяких нескінченних restart storms.
- Restart не повертає старі handle: cap generation invalidation, ресурсні claims звільняються, pending IPC відхиляється.
- Critical service failure -> safe degraded mode (GOP + serial; ніякого запису на диск за замовчуванням).
- Shutdown -> stop consumers before providers; reverse topological order, bounded timeouts, stale tasks marked faulted.
- Service-owned device access only via explicit handle with device identity, allowed operations і resource claims; driver crash не лишає DMA доступним після ревокації.
- User Task Manager показує **тільки справжні PID** з ProcessManager snapshot; service ID та PID — різні сутності; kernel audit події не редагуються службою.
- Спеціальні kernel critical loops не ховати як «невидимі служби»: в окремому kernel thread/device activity view.

## 7. Мінімальний набір тестів (можна писати одразу на host)

1. Два boot services запускаються у топологічному порядку за dependency graph.
2. Cycle `a->b->a` fail-closed, нуль side effects.
3. Missing hard dependency, duplicate ID, unknown capability/exec: reject.
4. Crash `a` не перепризначає PID `b`; service PID береться лише з ProcessManager.
5. Restart counter досягає limit -> `Failed`; clock simulated.
6. Unauthorized `service.stop`/device claim -> denied; invalid/revoked handle -> denied.
7. Snapshot є bounded/read-only; UI не може підмінити стан служби.
8. Graceful shutdown respects reverse dependencies.
9. Відсутність ізоляції у поточному build => `ServiceManager::execute_service` має повертати `Unsupported`, а не запускати код в Ring 0.

## 8. Порядок реалізації та зона агента

- **SVC-0:** документація + `src/services/service_manager.rs` pure graph/manifest/state machine і host tests, без редагування `src/main.rs`.
- **SVC-1:** mock event source + read-only service snapshots; жодного реального PID в UI без ProcessManager.
- **SVC-2:** після інтеграції Ring 3: verified native service entry, `spawn/exit`, bounded IPC і handles.
- **SVC-3:** перший справжній `zillad`, `deviced` і `logd`, тоді display service та інші.

**Власник коду:** окремий Services agent; Process agent володіє `src/kernel/process/**` і `src/services/process_manager.rs`; Graphics agent — `src/video/**` і `src/drivers/gpu/**`. Спільні `main.rs/lib.rs/Cargo.toml` змінювати лише маленьким інтеграційним diff після перечитування актуального HEAD.

Пов'язані: [OS subsystems](OS_SUBSYSTEMS.md), [Driver Framework](DRIVER_FRAMEWORK.md), [AMD GPU bring-up](AMD_GPU_BRINGUP.md), [Agent coordination](AGENT_COORDINATION.md).
