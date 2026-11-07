# ZillaOS

**ZillaOS** — незалежна експериментальна операційна система для **x86-64**,
з власним ядром **Rust `no_std`**, завантаженням через **Limine / UEFI**
і графічним робочим столом. Це **не** Linux-дистрибутив і **не** оболонка
поверх Windows. Ubuntu у WSL2 потрібна лише для збірки та тестів.

> **Поточний стан · 6 листопада 2026:** ZillaOS уже завантажувалася у
> VMware Workstation, показувала Desktop, меню Start, вікна, мишу,
> файловий менеджер, App Center і **Zilla Notepad**. Користувач
> підтвердив роботу блокнота та запису в RAMFS скриншотом.
>
> **Важливо:** сучасний Windows EXE, Linux ELF чи Android APK **ще не
> запускаються**. GPU acceleration, runtime зміна роздільної здатності,
> ізольовані Ring 3 процеси та постійна файлова система — окремі
> наступні етапи. Не плутати підготовлені структури даних із функціями,
> перевіреними у VM.

> **Почати нову хвилю після очищення чатів:** [Єдина інструкція старту](docs/NEW_CHAT_START_I_M2.md). Усі старі агентські PR злиті, 10 старих гілок видалені, нові агенти починають лише від свіжого `main`.

> **Інтеграція I-M1, 7 листопада 2026:** шість інженерних потоків зібрані в інтеграційний PR #19, підтверджені host/bare-metal/QEMU тести. Глобальний Rustfmt 1.90 виправлено окремим PR #26: повний integration CI та профільні workflows успішні. Новий порядок робіт, розмежування володіння кодом і правило «один `main` + окремі агентські PR» описані в [плані I-M2](docs/NEXT_WAVE_I_M2.md). Не вважай QEMU-докази перевіркою фізичного AMD GPU, справжнім SMP або запущеним `zillad`.

[<u>Поточна архітектура</u>](docs/ARCHITECTURE_CURRENT.md) ·
[<u>Правила агентів</u>](AGENTS.md) ·
[<u>Координація паралельних чатів</u>](docs/AGENT_COORDINATION.md) ·
[<u>Модель безпеки</u>](docs/SECURITY_ARCHITECTURE.md)

## Навіщо ми створюємо ZillaOS

Наша довгострокова мета — **Security-First, modular, multi-runtime OS**:

- **Власне ядро:** керування пам'яттю, розподіл ресурсів, процеси,
  системні виклики та згодом IPC/capabilities.
- **Звичний Desktop:** вікна, файли, Notepad, Task Manager, Settings,
  App Center та незалежні застосунки.
- **Єдина система запуску:** Start, Dock, Files і App Center використовують
  один `ApplicationManager`; реальне виконання буде делеговане
  `ProcessManager` після реалізації ізоляції.
- **Кілька runtime:** Zilla Native → Linux ABI/ELF → Windows PE/Win32 →
  Android runtime/VM → емулятори окремих консолей. Ці середовища
  **не** є взаємозамінними, у кожного власний ABI та залежності.
- **Безпека за замовчуванням:** user-mode ізоляція, least privilege,
  capabilities, W^X/NX, квоти, аудит, зрозумілий Task Manager. Це
  інженерні вимоги, а не обіцянка «100% немає вірусів».

## Що вже є у коді

| Підсистема | Фактичний рівень | Ще немає |
| --- | --- | --- |
| **Boot** | Limine/UEFI ISO, графічний boot у VMware | production installer |
| **Desktop** | Start, Dock, WindowManager, Settings, Files, UA/EN/RU, RTC, PS/2 ввід | user-space compositor/service |
| **App Center** | один реєстр вбудованих програм і відкриття view через AppManager | встановлення/запуск сторонніх binary |
| **Notepad** | введення ASCII, редагування й збереження `.TXT` у RAMFS | Unicode input, стале збереження |
| **Файли** | `/Program Files`, `/Users/Guest/Documents`, базове immutable дерево + bounded RAMFS | реальний VFS із дисковим записом |
| **Процеси** | PID, модель lifecycle/scheduler, плани адресного простору, validation-only syscalls | Ring 3, реальні CR3/syscall traps та ізоляція |
| **Windows PE** | PE32+ x64 inspector, host `pe-inspect` | PE mapping/relocations/imports/Win32 execution |
| **Відео** | RGB32 software renderer, GOP відеорежими, read-only VMware SVGA II PCI/BAR/register probe, FIFO-модель | runtime mode switch, FIFO commands, 2D/3D hardware acceleration |

Це **зріз стану**, а не автоматичний live status. Через паралельну роботу
агентів дивіться актуальний HEAD і профільні документи:
[ProcessManager](docs/PROCESSES.md),
[SVGA](docs/VMWARE_SVGA_DRIVER.md),
[Desktop](docs/DESKTOP.md),
[Application Framework](docs/APPLICATIONS.md).

## Швидкий запуск у Windows / VMware

**Середовище:** Windows 10/11, PowerShell 5.1+, WSL2 Ubuntu,
Rust toolchain 1.90.0, VMware Workstation. Builder перевіряє
залежності та генерує UEFI ISO.

```powershell
cd M:\projects\ZillaOs
git pull --ff-only origin main

# Перевірка WSL/Rust/xorriso та інших передумов
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Check

# Зібрати новий ISO і запустити VMware
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

Якщо `Check` повідомив про відсутні залежності:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Setup
```

`Setup` може попросити **пароль користувача Ubuntu у WSL**, не
Windows PIN. Перед повторною збіркою **повністю вимкніть** VM
(`Power Off`, не Suspend).

Готовий образ зазвичай: `dist/ZillaOS-Desktop-UEFI.iso`.
`-Action Run` запускає **наявний образ** і не компілює новий:
після змін коду потрібен саме `BuildRun`.

### Перевірка ядра й тестів в Ubuntu/WSL

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
python3 -m unittest discover -s tests -v
```

Це **host tests + компіляція**, не доказ того, що нова функція
працює всередині VM. Потрібна окрема runtime перевірка.

### Діагностика Windows EXE (аналіз, не запуск)

```bash
cargo +1.90.0 run --offline --target x86_64-unknown-linux-gnu \
  --features pe-inspector-cli --bin pe-inspect -- \
  /mnt/c/Windows/System32/notepad.exe
```

`pe-inspect` лише перевіряє PE32+ x86-64 заголовки. Він не
виконує `.exe`, не підвантажує DLL і не потрапляє до ISO за
замовчуванням.

## Організація репозиторію

```text
boot/                        Limine конфігурація / UEFI
scripts/                     WSL/PowerShell builder, ISO, VMware, тестові утиліти
src/
  main.rs                    ранній bootstrap ядра та GUI event loop
  lib.rs                     no_std API модулів
  kernel/                    memory, paging, exceptions, process models
  drivers/                   input, storage discovery, VMware SVGA probing
  video/                     video-capability contracts
  shell/                     desktop, windows, start, locale, icons
  apps/                      Notepad, App Center та єдиний registry
  services/                  ApplicationManager, ProcessManager gateway
  fs/                        immutable guest tree, bounded RAMFS
  compat/                    disabled foreign runtimes, Windows PE parser
  graphics.rs                поточний software framebuffer renderer
tests/                       host Python regression tests
docs/                        архітектура, безпека, agent rules, runbooks
.github/workflows/           GitHub CI для перевірок
```

Гостьові `/Program Files`, `/Users`, `/System` — **власні
віртуальні каталоги ZillaOS**, не копія `C:\Program Files`
Windows. Зараз користувацькі документи RAMFS зникають після reboot.

## Архітектура: сьогодні й завтра

```text
Сьогодні (вбудоване GUI в ядрі):
Desktop / Start / Dock / Files / Notepad / App Center
       |                          |
ApplicationManager          WindowManager
       |                          |
Built-in Rust views        RGB32 software renderer
       \__________________________/
                    |
               ZillaOS kernel
                    |
      Limine / UEFI / x86-64 + GOP framebuffer

Майбутня архітектура:
Native / Linux / Windows / Android apps
                   |
             ApplicationManager
                   |
      ProcessManager + IPC + capabilities
        |                         |
 ELF / PE / runtime           Window surfaces
        |                         |
   Protected kernel        Compositor / GPU driver
```

**Принцип:** процес має **одне джерело стану** в ProcessManager,
вікно — у WindowManager, запуск — через ApplicationManager,
фізичний display mode — через майбутній DisplayManager.
Не дублювати процеси, ресурси чи системні дозволи у GUI.

## Security-First Roadmap

1. **CPU isolation:** Ring 3, user address spaces, safe syscall
   entry, kill/exit, containment для #PF/#GP/#UD без kernel panic.
2. **Real ProcessManager:** PID/PPID, стан, runtime квоти,
   контрольоване завершення та одна таблиця задач.
3. **PermissionManager:** default-deny capabilities для файлів,
   мережі, пристроїв, камери, мікрофона й IPC.
4. **Task Manager / Security Center:** показувати реальні процеси,
   служби, їхні дозволи та kernel audit events. Не малювати
   вигадані CPU/RAM показники.
5. **Native user apps:** Zilla ABI + ELF loader, окремі процеси,
   потім Windows PE64 / DLL / Win32; після цього Linux ABI та
   Android VM/runtime.
6. **Graphics:** стабільний GOP fallback → безпечний SVGA FIFO →
   2D update/mode setting → окрема робота над 3D.

Наявність контролів у UI **не замінює** примусового захисту ядра.
Повна гарантія «вірусів не існує» неможлива.

Детальний план: [Security architecture](docs/SECURITY_ARCHITECTURE.md) ·
[Current architecture](docs/ARCHITECTURE_CURRENT.md) ·
[Compatibility](docs/COMPATIBILITY.md) ·
[Roadmap](docs/ROADMAP.md).

## Паралельна робота агентів

У проєкті зараз **два активні чати розробки**:

- **GPU/SVGA:** `src/drivers/gpu/**`, `src/video/**`, драйвер і
  режими екрана. [Документація](docs/VMWARE_SVGA_DRIVER.md).
- **Process/Windows:** `src/kernel/process/**`, `src/services/process_manager.rs`,
  `src/compat/**`, Ring 3 та Windows compatibility.
  [Документація](docs/PROCESSES.md).

Редагування `src/main.rs`, `src/lib.rs`, `Cargo.toml`,
`src/shell/desktop.rs` та інших спільних файлів допускається лише
малими сумісними змінами поверх **актуального HEAD**.
Force push, стирання змін іншого агента й дублювання системних
менеджерів заборонені.

**Агенте, почни з [AGENTS.md](AGENTS.md) і
[AGENT_COORDINATION.md](docs/AGENT_COORDINATION.md).**

## Документація

| Тема | Файл |
| --- | --- |
| Структура й фактичний стан ядра | [ARCHITECTURE_CURRENT.md](docs/ARCHITECTURE_CURRENT.md) |
| Довгостроковий дизайн мікроядра | [ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Рішення ADR | [DECISIONS.md](docs/DECISIONS.md) |
| Threat model / Task Manager / дозволи | [SECURITY_ARCHITECTURE.md](docs/SECURITY_ARCHITECTURE.md) |
| Правила кількох агентів | [AGENT_COORDINATION.md](docs/AGENT_COORDINATION.md) |
| Застосунки та запуск | [APPLICATIONS.md](docs/APPLICATIONS.md) |
| Вбудований блокнот | [NOTEPAD.md](docs/NOTEPAD.md) |
| Файлова модель | [FILESYSTEM.md](docs/FILESYSTEM.md) |
| Процеси та межі Ring 3 | [PROCESSES.md](docs/PROCESSES.md) |
| Windows/Linux/Android сумісність | [COMPATIBILITY.md](docs/COMPATIBILITY.md) |
| GPU/SVGA | [VMWARE_SVGA_DRIVER.md](docs/VMWARE_SVGA_DRIVER.md) |
| Windows PE, графічний план | [WINDOWS_GRAPHICS.md](docs/WINDOWS_GRAPHICS.md) |
| Windows Builder/VMware | [WINDOWS_ISO.md](docs/WINDOWS_ISO.md) |
| Desktop | [DESKTOP.md](docs/DESKTOP.md) |
| Build, tests, diagnostics | [BUILD.md](docs/BUILD.md), [VALIDATION.md](docs/VALIDATION.md) |

## Статус і відповідальність

ZillaOS — дослідницький проєкт, **не** готовий desktop OS та
**не** інструмент для безпечного виконання невідомих EXE на поточній
стадії. Не використовувати з реальними даними або на критичному
обладнанні без перевіреного backup та ізоляції.

**License:** у репозиторії поки немає кореневого `LICENSE` файла;
це потребує окремого рішення власника, перш ніж робити твердження про
open-source ліцензію.
