# ZillaOS — поточна архітектура та межі реалізації

**Дата зрізу:** 2026-11-06. **Гілка:** `main`. Паралельні GPU
і Windows/Process агенти змінюють код просто зараз: цей документ —
орієнтир, а **останній HEAD і тести завжди мають пріоритет**.

Документ відділяє **факт поточного коду**, **дизайн-політику** і
**цільову архітектуру**. Довгострокова специфікація:
[ARCHITECTURE.md](ARCHITECTURE.md); прийняті планові рішення:
[DECISIONS.md](DECISIONS.md).

## 1. Що реально працює, а що тільки підготовлено

| Підсистема | Перевірений / наявний рівень | Не плутати з |
| --- | --- | --- |
| Boot | Limine + UEFI, x86-64 Rust `no_std`; ISO в VMware, показано GUI | Linux kernel, власний UEFI firmware |
| Desktop | вікна, Start, Dock, Files, Settings, App Center, миша, RTC, локалізація UA/EN/RU | ізольований desktop service |
| Notepad | редагування ASCII і volatile збереження `.TXT` у RAMFS, підтверджене скриншотом користувача | стале збереження на диск |
| Files | immutable guest tree + лімітований volatile RAMFS | NTFS/FAT32 writer чи доступ до файлів Windows |
| ApplicationManager | один registry вбудованих views і спільний open/restore | завантаження native processes |
| ProcessManager | PID, стани, scheduler selection, memory plans, validation-only syscall model | CR3 switch, Ring 3, syscall trap, real process |
| Windows PE | x86-64 PE32+ parser і host `pe-inspect` | IAT loader, Win32 API або запуск `.exe` |
| Graphics | Limine framebuffer, CPU renderer; меню UEFI video modes | runtime mode setting, SVGA3D |
| VMware SVGA | read-only PCI/BAR/register probe, FIFO ring arithmetic tests | MMIO FIFO mapped, GPU commands/acceleration |
| Security | модель прав memory regions, fail-closed foreign execution, документація загроз | перевірений kernel sandbox, антивірус |

У CI Rust unit tests і bare-metal build **не підтверджують**
виконання Ring 3, GPU acceleration або реальний ISO boot для кожного
нового коміту. Всі зміни в runtime потребують окремої VMware/QEMU
перевірки.

## 2. Архітектурні шари

```text
                    [ нині: embedded kernel GUI ]
  Desktop / Start / Dock / Files / Notepad / Settings / App Center
                  |                        |
             ApplicationManager       WindowManager
                  |                        |
             built-in views            UI renderer
                  |                        |
                Rust no_std kernel ---- GOP framebuffer
                  |                  \
           memory/paging               GPU SVGA probe (read-only)
                  |
         process metadata + syscall policy
                  |
           Limine / UEFI / x86-64
```

```text
                     [ цільова архітектура ]
     User apps (Native / Linux / Windows / Android guest runtime)
                  |                |
              App Broker      Window surfaces
                  |                |
           ProcessManager     WindowManager
                  |                |
       Syscall / IPC / caps  Compositor / DisplayManager
                  |                |
             Memory / VFS    VMware SVGA / drivers
                  +--------+-------+
                           |
                    Protected Kernel
                           |
                     UEFI / x86-64
```

У верхньому target-діаграмі **немає** твердження, що Linux ABI,
Win32, Android runtime, гіпервізор або desktop IPC вже працюють.

## 3. Структура коду і власники стану

```text
src/
  main.rs                   boot, early event loop, diagnostics
  lib.rs                    публічні no_std модулі
  kernel/
    memory.rs               рання фізична пам'ять
    paging.rs               paging structures
    exceptions.rs           CPU diagnostics
    process/                PID/state/memory plans, scheduler/syscall validation
  services/
    app_manager.rs          запуск зареєстрованого embedded view
    process_manager.rs      service gateway для процесних планів
  shell/
    desktop.rs              UI state + input routing
    window_manager.rs       єдиний owner вікон і focus/z-order
    locale.rs               локалізація UI
    start_menu.rs           геометрія меню
  apps/
    mod.rs                  registry додатків
    notepad.rs              bounded text editor
  fs/
    mod.rs                  immutable virtual tree
    ramfs.rs                volatile запис документів
  compat/
    mod.rs                  disabled foreign runtimes
    windows_pe.rs           безпечна перевірка PE32+ headers
  video/
    mod.rs                  video capabilities / SVGA integration
  drivers/gpu/vmware_svga/
    pci.rs                  PCI/BAR verification
    registers.rs            регістри SVGA
    capabilities.rs         обережне читання характеристик
    fifo.rs                 pure FIFO model, no GPU writes
  graphics.rs               early GUI framebuffer renderer
  pci_io.rs                 low-level IO adapter (в ядрі)
```

У реальному репозиторії з часом можуть з'являтися додаткові файли;
цей список не використовується як автоматична схема генерації.

### Single source of truth

- PID, parent/child, exit/fault — **ProcessManager**.
- App registry/launch request — **ApplicationManager**.
- Window lifecycle, z-order, focus, geometry — **WindowManager**.
- Physical display, mode, pitch, backing buffer — майбутній **DisplayManager**.
- Permissions/capabilities — майбутній **PermissionManager** через
  syscall/resource boundary, без UI-only політики.
- File ownership — майбутній **VFS**, у v0 лише immutable tree + RAMFS.
- System events — майбутній структурований EventLog, а не «список
  процесів» в окремому Task Manager.

## 4. Архітектурні питання та прийняті напрямки

### Q1. Чи це Linux?

**Ні.** Власне Rust-ядро стартує через Limine/UEFI.
Windows/WSL Ubuntu компілює код та ISO, але не виконується в гостьовій
ZillaOS після boot. Linux ABI — окрема майбутня підсистема.

### Q2. Який executable першим запускати?

**Власний Zilla Native** test fixture у Ring 3 з контрольованим
`write/exit` і fault containment. Після цього — validated ELF loader,
потім мінімальний Win32 PE64 console Hello World, лише необхідні
`ntdll/kernel32` exports. APK, Waydroid/Android, консолі — окремі
масштабні runtime/virtualization/emulation цілі.

### Q3. Який тип ядра?

Мета — **capability-oriented microkernel**. Поточна GUI-demo
вбудована в ядро і не є мікроядром у повному сенсі. Системні
служби можуть бути перенесені в user mode після IPC/capabilities.

### Q4. Чи потрібні демони/служби?

Так, але після захищених процесів. `ServiceManager`/supervisor
керуватиме restart і dependencies; `SecurityMonitor` отримуватиме
read-only audit події; `TaskManager` показуватиме реальний
ProcessManager snapshot. Не створювати привілейований моноліт.

### Q5. Як запускати Windows/Linux/Android в одному Desktop?

Спільні AppManager/ProcessManager/WindowManager, **окремі**
binary loader, ABI translator або гостьова VM, common capabilities
і surface protocol. Підтримка файлу-формату не дорівнює підтримці
системного API. Усі sandbox політики мають діяти незалежно від
походження програми.

### Q6. Як розвивати GPU без поломки Desktop?

GOP CPU renderer — стабільний fallback. Спочатку BAR/register
probe, потім bounded MMIO/FIFO із негативними тестами, 2D updates,
mode switch + rollback, після цього графічне прискорення;
SVGA3D — окремий етап. GPU agent не змінює процесні PTE напряму.

### Q7. Як уникнути вірусів і «прихованих процесів»?

Замість обіцянки нуль malware: kernel-enforced least privilege,
Ring 3, W^X/NX, capabilities, квоти, структурований audit,
allowlisted autostart, контроль package provenance. Task Manager
читає **єдину** kernel process table. Якщо сам kernel/firmware
скомпрометовано, повна видимість не гарантується.

### Q8. Як зберігати файли?

Нині `/Users/Guest/Documents` — RAMFS. Пізніше VFS → read-only
block driver → тестований filesystem writer → durable storage.
Драйвер не повинен писати на хостовий диск без opt-in.

## 5. Integration milestones та докази

1. **Process v0 (модель):** PID/state, address-space plan, syscall
   argument validation і тести. Це поточний рівень.
2. **Process v1 (виконання):** Ring 3 fixture, адресні простори,
   read/write permissions, exception containment, exit; перевірка в VM.
3. **Security v1:** capability-checked file and IPC operations, audit log,
   Task Manager на реальних PID. Доказ — негативні isolation tests.
4. **GPU v1:** безпечні апаратні регістри, FIFO/2D commands із fallback.
   Доказ — mode/framebuffer tests + VM capture.
5. **GPU v2:** runtime mode switch із rollback, далі прискорення;
   не відмічати «активне» до реального підтвердження.
6. **Compat v1:** native ELF, потім PE64 + DLL imports/Win32 ABI;
   контрольований Windows console EXE у user mode.
7. **Compat v2+:** Linux ABI, Android VM/runtime, специфічні console
   emulators, тільки після базової security моделі.

## 6. Оновлення документації при паралельній роботі

GPU agent пише у `VMWARE_SVGA_DRIVER.md`, Runtime agent — у
`PROCESSES.md`. Коли етап проходить, додавайте факт і test evidence
до відповідного документа. README лишається верхнім коротким зведенням;
оновлювати його треба лише з підтвердженої інформації.

Технічні правила двох агентів: [AGENTS.md](../AGENTS.md),
[AGENT_COORDINATION.md](AGENT_COORDINATION.md). Безпека:
[SECURITY_ARCHITECTURE.md](SECURITY_ARCHITECTURE.md).
