# AGENTS.md — правила роботи AI-агентів у ZillaOS

> **Прочитай перед будь-якою зміною коду.** Це експериментальна самостійна ОС на Rust `no_std`, x86-64/UEFI/Limine. Ubuntu/WSL — лише середовище збірки, **не** ядро ZillaOS.
>
> **Робоча гілка користувача — `main`.** Паралельно працюють щонайменше два агенти: **Graphics/SVGA** та **Process/Windows Runtime**. Третій потік може змінювати документацію. Не припускай, що HEAD залишився незмінним після попередньої операції.

## 1. Перед початком роботи

1. Прочитай `README.md`, `docs/AGENT_COORDINATION.md`, `docs/ARCHITECTURE_CURRENT.md` та `docs/SECURITY_ARCHITECTURE.md`.
2. Перед змінами у своїй підсистемі переглянь її актуальні файли **саме з HEAD `main`**, відповідні тести й документацію. Старий контекст чату — не джерело істини.
3. Переконайся, що розумієш межу між **host unit test**, **bare-metal build**, **UEFI boot у VMware** та **реальною роботою функції**. Це різні рівні підтвердження.
4. Працюй малими, логічними комітами з обов'язковим read-back. Не переписуй чужу роботу заради «вирівнювання» структури.

## 2. Межі володіння кодом

**Graphics/SVGA agent (дисплей):**
- Основна зона: `src/drivers/gpu/`, `src/video/`, документація `docs/VMWARE_SVGA_DRIVER.md`, `docs/GRAPHICS.md`.
- Задачі: PCI/SVGA discovery, безпечна ідентифікація ресурсів, FIFO, DisplayManager, compositor, runtime mode switching, графічні тести.
- **Не торкайся** `src/kernel/process/`, PE loader, Win32 API або ProcessManager без погодженої інтеграції.

**Process/Windows Runtime agent (виконання):**
- Основна зона: `src/kernel/process/`, `src/services/process_manager.rs`, `src/compat/`, майбутні `src/loaders/` та `src/runtimes/`, документація `docs/PROCESSES.md`, `docs/COMPATIBILITY.md`.
- Задачі: адресні простори, Ring 3, GDT/TSS, syscall/trap, fault isolation, native ELF, потім PE64 і Win32 API.
- **Не торкайся** GPU BAR, FIFO, framebuffer, SVGA-драйвера або DisplayManager.

**Documentation/architecture agent (цей потік):**
- Основна зона: `README.md`, `AGENTS.md`, `docs/AGENT_COORDINATION.md`, `docs/ARCHITECTURE_CURRENT.md`, `docs/SECURITY_ARCHITECTURE.md`.
- Не вносить зміни в Rust/boot/скрипти, якщо немає окремого запиту користувача.

**Спільні файли** (`src/main.rs`, `src/lib.rs`, `Cargo.toml`, `src/shell/desktop.rs`, `src/graphics.rs`, `src/services/app_manager.rs`, workflows): тільки мінімальні сумісні правки з повторним читанням поточної версії перед записом. Не проводь широких рефакторингів у цих файлах паралельно.

## 3. Конкурентні коміти до main

- Перед кожним записом перевір актуальний HEAD і версію/ SHA файла, який змінюєш.
- Використовуй **non-force** update з очікуваним попереднім HEAD (compare-and-swap) або GitHub Contents API з актуальним blob SHA. Якщо HEAD змінився — перечитай, перезастосуй **лише свій diff**, перевір і повтори.
- **Заборонено:** force push, reset/checkout із втратою чужих змін, автоматичний overwrite, видалення чужих модулів, тривалий локальний snapshot як основа запису без оновлення.
- У разі конфлікту в спільному API: спершу документуй мінімальний контракт, погоджуй черговість змін; не створюй власний дубль ProcessManager, WindowManager, AppManager, frame allocator або framebuffer.
- Не запускай одночасно два процеси редагування того самого файла. **Коміт не є автоматично підтвердженням працездатності.**

## 4. Архітектурні інваріанти та безпека

- Жоден сторонній ELF/PE/APK не виконується в Ring 0. Неперевірені завантажувачі fail-closed.
- `ProcessManager` зберігає процеси; `ApplicationManager` маршрутизує запуск; `WindowManager` володіє станом вікон. Це **різні** обов'язки.
- PID metadata і таблиця регіонів пам'яті **не доводять** наявність Ring 3, реального CR3, syscall trap або ізоляції.
- Read-only PE inspection **не дорівнює** виконанню EXE; PCI probe **не дорівнює** GPU acceleration.
- Kernel/user межа, перевірка довжин/адрес, W^X/NX, відсутність несанкціонованого доступу до файлів/пристроїв — обов'язкові для майбутнього запуску чужих програм.
- Жодного запису на реальні диски або зміни VM firmware/NVRAM без окремого схвалення; випробування дискових операцій тільки на disposable images.
- `unsafe`, MMIO, port I/O, page-table writes і trap handlers вимагають документованих інваріантів, негативних тестів і безпечного fallback.

## 5. Перевірки й критерії готовності

Для змін Rust за наявності toolchain:

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
python3 -m unittest discover -s tests -v
```

Windows/WSL збірка у користувача:

```powershell
cd M:\projects\ZillaOs
git pull --ff-only origin main
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

Не стверджуй, що `fmt`, CI, Windows збірка чи запуск VMware пройшли, якщо цього не перевіряли. CI, який тригериться тільки на `src/**`, може **не запускатися** після суто документаційного коміту — це нормально, але не називай його «зеленим для цього коміту».

## 6. Формат звіту агента

У кінці кожного етапу вкажи:
- зміни і точні шляхи файлів;
- commit SHA/посилання у `main`;
- що перевірено (тести, compile, VM) з доказом;
- чого **ще немає**;
- ризики/блокери, взаємодію з другим агентом;
- 1–3 конкретні наступні кроки.

**Посилання:** [координація](docs/AGENT_COORDINATION.md) · [поточна архітектура](docs/ARCHITECTURE_CURRENT.md) · [безпека](docs/SECURITY_ARCHITECTURE.md) · [запуск програм](docs/APPLICATIONS.md) · [процеси](docs/PROCESSES.md) · [SVGA](docs/VMWARE_SVGA_DRIVER.md).
