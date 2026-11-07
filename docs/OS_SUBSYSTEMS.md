# ZillaOS — підсистеми ОС та межі відповідальності

> **Статус: проєктна архітектура, не готова реалізація.** Зріз репозиторію `kabasik007/ZillaOs`, `main` на 2026-11-07, базовий commit `12d80ada4d7d1b712f55b15e0efb49d5ac18c4ca`. У паралельних агентів може бути новіший HEAD. Перед редагуванням перечитати `AGENTS.md` і `docs/AGENT_COORDINATION.md`.

## 1. Факти, виявлені в репозиторії

- `src/main.rs`: boot/demo, early I/O, serial, storage/video probe, desktop event loop. Це наразі великий інтеграційний вузол.
- `src/kernel/{memory,paging,process}`: memory/paging та моделі PID/thread/scheduler; `src/services/process_manager.rs` ще не є повноцінним ізольованим процесним сервісом. Є opt-in тестові Ring 3 fixtures; не плутати їх із загальним user process runtime.
- `src/services/{app_manager,process_manager,task_snapshot}`: Rust-модулі/фасади, а не окремі user-space daemon executables.
- `src/video/display_manager.rs`: уже визначає `ModeBackend`, confirmed/pending mode та rollback, але фізичний mode-switch backend ще відсутній.
- `src/drivers/gpu/vmware_svga/*`: PCI discovery, реєстри, negotiating opt-in, pure FIFO/queue та MMIO preflight. Не є працюючим GPU acceleration.
- PCI відкриття розкидане між `src/drivers/storage_pci.rs`, `src/video/mod.rs`, `src/drivers/gpu/vmware_svga/pci.rs`, а доступ до CF8/CFC — у `src/pci_io.rs`. Це перша точка для узгодженого read-only PCI API.
- `src/drivers/mod.rs` зараз експортує лише `rtc`; інші драйвери підключаються через path modules. Не робити масштабне переміщення під час паралельної роботи GPU/Process агентів.
- Desktop, WindowManager, software framebuffer та RAMFS — окремі компоненти, але більша частина ще виконується в контексті ядра.

## 2. Позиціонування ядра

Ціль із `docs/DECISIONS.md`: **capability-oriented microkernel**, з мінімальним привілейованим ядром та ізольованими службами/драйверами, коли будуть готові Ring 3 + IPC + capabilities. **Сьогодні ZillaOS — kernel-embedded desktop prototype**, не завершений мікроядерний продукт.

```text
USER MODE (ціль)
  Desktop / App Center / Settings / Task Manager / Native apps
          |              |                 |
     ApplicationManager  WindowManager  ServiceManager(zillad)
          |              |                 |
        IPC + versioned capability handles (ще немає)
======================= trust boundary =======================
KERNEL (поточний / еволюційний)
  Process/Thread Scheduler | Memory/Paging | Interrupt/Syscall/IPC
  Capability enforcement  | Device Registry | Driver resource broker
  PCI/ACPI/IRQ/DMA/Timers  | Display bootstrap | VFS/block substrate
======================== hardware ============================
  CPU / RAM / PCIe / firmware / AMD GPU / VMware SVGA / disks
```

**Незмінні власники стану:** PID — ProcessManager; application routing — ApplicationManager; вікна — WindowManager; physical display/mode — DisplayManager; фізичні сторінки — один frame allocator; пристрої та BAR/IRQ/driver binding — DeviceManager; перезапуски служб — ServiceManager. Жодна нова служба не створює паралельний реєстр того самого ресурсу.

## 3. Функціональні «відділи» ОС

| Відділ / зона | Kernel mechanism (мінімум) | Майбутня служба / API | Перший реальний результат |
| --- | --- | --- | --- |
| Boot & platform | Limine, CPU init, HHDM, interrupt routing, ACPI validation | boot diagnostics, platform info | стабільний boot + serial |
| CPU / process | page-table owner, Ring 3, TSS, syscall, scheduler, faults | ProcessManager API | isolated native fixture, teardown |
| Memory | PMM, VMM, MMIO map/unmap, frame accounting | memory statistics provider | audited mappings з охороною ресурсів |
| Bus / devices | PCI config, bridge enumeration, resource tree, IRQ, claim/release | DeviceManager / `deviced` (future) | єдиний read-only PCI snapshot |
| Graphics | GOP fallback, surface ownership, compositor plumbing | DisplayManager / `displayd` (future) | узгоджений `VideoAdapter` + mock AMD |
| Storage | block device contract, read-only IO, VFS | `storaged` / mount broker (future) | read-only disposable disk |
| Input | PS/2 bootstrap, bounded event queue | `inputd` (future) | узгоджений keyboard/mouse API |
| Network | NIC isolation + packet buffers (future) | `netd` (future) | loopback/mocked stack до NIC DMA |
| Audio | audio device substrate (future) | `audiod` (future) | mock sink, згодом HDA |
| Security | kernel capability check, audit emission | PermissionManager, `securityd` | deny-by-default test |
| Service orchestration | verified launch/stop, watchdog primitives | `zillad` | детермінований graph startup на mock |
| Power / hotplug | ACPI, sleep state machine (future) | `powerd`, DeviceManager events | bounded suspend/resume prototype |
| Files / apps / UI | handle rights, IPC, event transport | AppManager, Shell, TaskManager | жодних власних device registers у UI |

Назви служб — **проєктні**. Не позначати їх як уже запущені демони.

## 4. Запропонована структура, створювати поступово

```text
src/
  kernel/                 # уже є; додавати точкові механізми, не переносити UI відразу
    process/              # існує, зона Process агента
    hal/                  # ПРОПОЗИЦІЯ: cpu, irq, timers, acpi, mmio
    device/               # ПРОПОЗИЦІЯ: registry, claims, resource ownership
    dma/                  # ПРОПОЗИЦІЯ: DMA mapping + IOMMU policy
    ipc/                  # ПРОПОЗИЦІЯ: handles, queues, ABI version
  bus/                    # ПРОПОЗИЦІЯ: pci discovery and descriptors (pure core first)
    pci/
  drivers/
    gpu/
      vmware_svga/        # існує; не переписувати
      amd/                # ПРОПОЗИЦІЯ: AMD discovery first, no hardware writes
    storage/              # згодом перенести без breaking imports
    input/
  video/
    display_manager.rs    # існує — спільний DisplayManager
  services/               # існує; поки kernel-linked preview modules
    service_manager.rs    # ПРОПОЗИЦІЯ: host-testable supervisor model
    device_manager.rs     # ПРОПОЗИЦІЯ: discovery/bind policy facade
    security_manager.rs   # ПРОПОЗИЦІЯ: permissions; лише після enforcement
  shell/                  # існує, UI без роботи з PCI BAR
  apps/                   # існує
  fs/                     # існує
  compat/                 # існує
```

**Важливо:** структура вище — план, а не список створених директорій. Спершу окремі pure Rust модулі; лише потім інтеграція у спільні `src/lib.rs`, `src/main.rs`, `Cargo.toml`.

## 5. Стартовий порядок і залежності

```text
Boot + serial + GOP
   ├── CPU interrupts + allocator + paging
   │       ├── Process isolation ── IPC ── permissions ── real user services
   │       └── safe MMIO mapper ─┐
   ├── PCI scan + resource registry ── DeviceManager ── Driver binding
   │                                └── DMA/IOMMU/IRQ before active GPU
   └── DisplayManager + GOP fallback ── VMware backend / AMD discovery
                                           └── AMD DCN -> later acceleration
```

**Старт boot:** CPU/memory/serial → firmware GOP → read-only PCI inventory → passive device binding → desktop. Ранній `DeviceManager` може працювати в ядрі як статична модель; **жоден user-space daemon не запускається без ізоляції та IPC**.

## 6. Інваріанти інтеграції та безпеки

1. **No blind MMIO:** регістри не читати/писати без перевіреного BAR aperture, mapping, alignment, byte bounds та ownership claim.
2. **No unsolicited DMA:** PCI bus mastering заборонено вмикати до підготовленого DMA mapping API та рішення щодо IOMMU/isolation. CPU physical address не вважати DMA address.
3. **Keep GOP fallback:** відмова AMD/VMware не повинна прибирати базовий робочий Desktop.
4. **Ring 0 is trusted only:** майбутні сторонні драйвери — окремі ізольовані сервіси або явно аудитовані привілейовані компоненти.
5. **No invented telemetry:** ніяких «GPU 99%», CPU% або MB VRAM у Task Manager без лічильників з реального джерела.
6. **No magic AMD register offsets:** register maps/version matching тільки після вибору конкретного PCI Device ID та IP block revision.
7. **Bounded loops:** PCI scan, firmware parsing, retry, interrupt processing та ring queues мають caps/timeouts й error fallback.
8. **Single writer/shared interfaces:** `main.rs`, `pci_io.rs`, `lib.rs`, `video/mod.rs`, `Cargo.toml` — інтеграційні файли; узгоджувати зміни з існуючим GPU та Process потоками.

## 7. Етапи до справжньої модульності

- **M0 (документація + pure mocks):** архітектурні контракти, mock PCI inventory, DriverMatch/DriverProbe (без апаратних записів), ServiceManifest validation.
- **M1 (read-only device core):** read-only PCI snapshot із stable BDF/IDs + device claims; VMware SVGA і AMD побачені через спільний inventory, не через копії сканера.
- **M2 (kernel resources):** validated MMIO mapper, IRQ ownership, DMA policy; negative tests, no raw driver-side mapping.
- **M3 (user isolation):** підтверджений Ring 3, syscall/IPC, cap handles; real service process supervisor.
- **M4 (GPU/display):** firmware/IP-specific AMD DCN bring-up на конкретній платі, rollback, лише після цього queues/2D/3D.
- **M5 (SMP):** per-CPU scheduling, locking, TLB shootdown, IRQ affinity, race/fault injection. Не називати модель `MAX_CPUS=8` реалізацією SMP.

## 8. Еталон приймання

- Окремо вказати: **source written / host unit-tested / bare-metal built / QEMU-booted / VMware-verified / real-AMD-hardware-verified**.
- Для pure changes: `cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline`, `cargo +1.90.0 fmt --all -- --check`.
- Для інтеграції: `cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo`, `python3 -m unittest discover -s tests -v`, потім boot + COM1.
- No hardware evidence = no "AMD driver works" claim.
- Код має лишатися сумісним з експериментальним Windows/WSL → ISO → VMware workflow; native AMD GPU потребує **окремого** заліза або належно налаштованого passthrough.

Суміжні документи: [служби](SERVICE_FRAMEWORK.md), [драйвери](DRIVER_FRAMEWORK.md), [AMD bring-up](AMD_GPU_BRINGUP.md), [поточний стан](ARCHITECTURE_CURRENT.md), [координація агентів](AGENT_COORDINATION.md).
