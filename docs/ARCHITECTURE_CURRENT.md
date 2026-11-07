# ZillaOS — поточна архітектура (I-M2 baseline)

**Дата зрізу:** 2026-11-07. Відображає об'єднаний код після PR #19 і форматування PR #26. Для актуального фактичного стану дивись [STATUS](STATUS.md) і перевір поточний `main`. Цей документ пояснює **де проходять межі**, а не обіцяє, що всі target-підсистеми вже працюють.

## 1. Що виконується в гостьовій ОС сьогодні

```text
UEFI firmware
    → Limine boot protocol
    → src/main.rs: _start / memory-map / fault handlers
    → early memory + paging helpers / serial logs
       ├─ normal/feature-selected test boot
       ├─ vm-demo: Setup → embedded Shell / Desktop
       │            ├─ WindowManager (вікна, фокус, з-order)
       │            ├─ ApplicationManager (built-in apps only)
       │            ├─ graphics.rs CPU software rendering → GOP framebuffer
       │            └─ BootPci → single read-only DeviceRegistry
       └─ opt-in native-ring3 experimental QEMU fixtures
            ├─ ring3-guard-fault / supervisor-access-denial
            ├─ two user threads (cooperative)
            └─ timer preemption regression
```

**Не змішувати:** `native-ring3*` — експериментальний QEMU execution path, а Desktop views сьогодні ще не є системою довільних ізольованих user-space процесів. `ServiceManager`, `ProcessManager`, `IPC`, `DeviceRegistry`, `DisplayManager` мають моделі й контракти, але лише частина з них пов'язана з boot path, а не з production runtime.

## 2. Канонічні модулі й власники

| Власник/межа | Канонічні вихідні файли | Поточний рівень |
| --- | --- | --- |
| Kernel entry/boot | `src/main.rs`, `src/limine.rs`, `src/serial.rs` | boot, diagnostics, opt-in fixtures |
| Memory/paging/fault models | `src/kernel/memory.rs`, `paging.rs`, `page_table_walk.rs`, `exceptions.rs` | host tests, boot/opt-in runtime gates |
| ProcessManager / CPU0 lifecycle | `src/kernel/process/**`, `src/services/process_manager.rs` | bounded PID/TID/cleanup/scheduler models; native Ring3 fixtures separate |
| IPC / capability authority | `src/kernel/ipc/**`, `src/kernel/capabilities/**` | typed models, no general user syscall boundary |
| Service supervision | `src/services/service_manager.rs`, `service_supervisor.rs` | model-only, no user-space daemon |
| DeviceRegistry / PCI | `src/platform/**`, `src/pci_io.rs` | boot-time single read-only PCI segment-0 inventory |
| DisplayManager | `src/video/display_manager.rs`, `display_contracts.rs` | fail-closed host-tested contracts |
| GPU adapters | `src/drivers/gpu/vmware_svga/**`, `amd/**` | read-only/device-diagnostic + pure models |
| WindowManager | `src/shell/window_manager.rs` | embedded GUI state (not task registry) |
| ApplicationManager | `src/services/app_manager.rs`, `src/apps/**` | launch built-in views, deny foreign runtime |
| FS and installer | `src/fs/**`, `scripts/installer_*.py` | RAMFS and disposable/test-only geometry/ESP |

**Single source of truth:** не створювати другий PID/ProcessTable, другий PCI scanner/DeviceRegistry, другий WindowManager чи другий DisplayManager. ApplicationManager не має виконувати сторонні binary в Ring 0. ServiceId ≠ PID; права на ресурс ≠ текстова мітка в маніфесті.

## 3. Platform read-only device flow

```text
src/main.rs (vm-demo)
    → pci_io::discover_boot()
        → LegacyConfig CF8/CFC READ traversal (segment 0)
        → bounded temporary PciInventory
        → BootPci + DeviceRegistry (generation-safe handles)
            ├─ Storage snapshot
            ├─ Video / VMware SVGA cached PCI identity
            └─ AMD diagnostic via platform/amd_bridge.rs
```

Система **не** вмикає PCI bus mastering, BAR sizing, DMA, GPU MMIO/IRQ або активну 2D/3D чергу. `BootPci` не є production multi-segment PCIe ECAM/hotplug manager. Деталі: [Platform I-M1](PLATFORM_IM1_SINGLE_INVENTORY.md), [AMD](AMD_GPU_A0_IMPLEMENTATION.md), [Display contracts](GRAPHICS_DISPLAY_CONTRACTS.md).

## 4. Process / IPC / Services — межа інтеграції

```text
Trusted CPU-local current task identity (ProcessManager)
       │
       ├─ PID/TID / exit ticket / cleanup phases (host-tested model)
       │           │
       │           ├─ capability HandleTable, revoke, IPC queue/call cleanup
       │           └─ typed ServiceSupervisor notification and restart gating
       │
       └─ FUTURE real trap/syscall → user request copy/validate → kernel checks
```

Реальний production entry point має перевіряти ID виклику **в ядрі**, а не довіряти `sender_pid` з IPC payload. Моделі cleanup не повинні видаватися за реальне звільнення фізичної пам'яті/IRQ/DMA. Порядок і тести: [Process I-M1](PROCESS_LIFECYCLE_IM1.md) · [IPC foundation](IPC_CAPABILITIES_FOUNDATION.md) · [Services bridge](SERVICE_MANAGER_PROCESS_BRIDGE.md).

## 5. Цільова архітектура — НЕ реалізована повністю

```text
Native apps / future Linux ABI / future Win32 / future Android VM
                          │
                 ApplicationManager
                          │
                Protected ProcessManager
                  │                  │
              Syscall / IPC      UI surface handles
                  │                  │
             Capabilities         Compositor
                  │                  │
             DeviceRegistry     DisplayManager
                  └──────────────┬───┘
                         drivers / VFS
                          │
                   protected kernel
```

ZillaOS планує capabilities-first **модульну ОС** з мінімізацією довіреного коду. Це архітектурний напрямок, а не твердження про вже готове microkernel-split user space. Реальне ізолювання драйверів і служб перевіряється окремо, не тільки існуванням їхніх структур даних.

## 6. Інваріанти і workflow

1. Ніяких довільних ELF/PE/APK в Ring 0; user-space execution — лише після доведених memory/syscall isolation gates.
2. Усі handle/resource права перевіряються в авторитетній межі використання; stale/generation не можна відновлювати.
3. Зміни GPU і PCI, які пишуть у hardware, заборонені без окремого lab review, invariant proof та fallback.
4. Звичайний Desktop має зберігати CPU framebuffer fallback при невідомому GPU.
5. Спільні `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `platform/amd_bridge.rs` узгоджує тільки Chief Architect.
6. Для змін поведінки спочатку [специфікація](specs/README.md), потім PR/тести. Плани — [I-M2](NEXT_WAVE_I_M2.md), докази — [EVIDENCE_MATRIX](verification/EVIDENCE_MATRIX.md).

Історична архітектурна еволюція й ADR: [ARCHITECTURE.md](ARCHITECTURE.md), [DECISIONS.md](DECISIONS.md). Не оновлюй цей документ числами CI без посилання на перевірений commit/run.
