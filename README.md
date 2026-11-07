# ZillaOS

**ZillaOS** — експериментальна незалежна операційна система для x86-64: власне ядро на Rust `no_std`, завантаження через **Limine / UEFI**, графічний Desktop і поступовий перехід до ізольованих процесів, IPC та драйверів.

**Це не Linux-дистрибутив і не готова заміна Windows.** Windows/WSL2 використовується лише як середовище розробки; сторонні Windows EXE, Linux ELF та Android APK поки **не запускаються** як повноцінні застосунки ZillaOS.

> **Статус: experimental / development · зріз 7 листопада 2026.** Інтеграцію шести інженерних напрямків завершено в [PR #19](https://github.com/kabasik007/ZillaOs/pull/19); форматування Rust 1.90 виправлено в [PR #26](https://github.com/kabasik007/ZillaOs/pull/26). На перевіреній версії PR #26 пройшли Rust/Python тести, bare-metal збірки, QEMU/OVMF і дев'ять профільних/інтеграційних workflow. Після цього в `main` оновлювалася документація. Це **не доказ** production security, фізичної AMD, апаратного GPU-прискорення, повного SMP чи виконання `zillad`.
>
> **Почніть з [документаційного навігатора](docs/README.md)**. Для точних рівнів готовності та доказів: [STATUS](docs/STATUS.md). Для нових агентів: [AGENTS.md](AGENTS.md) і [I-M2 Mission Control](https://github.com/kabasik007/ZillaOs/issues/25).

## Що вже є

| Підсистема | Фактична наявність / підтвердження | Межа реалізації |
| --- | --- | --- |
| **Завантаження** | Limine/UEFI ISO, Rust `no_std`, QEMU/OVMF smoke, серійна діагностика | Не production installer, не власна прошивка UEFI |
| **Desktop** | Програмний framebuffer, Start/Dock, WindowManager, Settings, Files, App Center, миша, локалізація, Notepad; попередній запуск у VMware підтверджено користувачем | GUI все ще вбудований у ядро; немає user-space compositor |
| **Файли / інсталятор** | Обмежена RAMFS, read-only GPT/дискові моделі та disposable installer labs | Немає безпечного production-драйвера запису на фізичний диск |
| **Процеси** | Process/Thread lifecycle, CPU0 scheduler і cleanup-моделі; **окремі opt-in Ring 3 / fault / preemption QEMU fixtures** | Звичайні Desktop-вікна не є ізольованими процесами; повний SMP відсутній |
| **IPC / capabilities** | Типізовані об'єкти, generation-safe handles, права та host-tested негативні сценарії | Немає завершеного production IPC/syscall boundary |
| **Служби** | ServiceManager/Supervisor як `no_std` модель, dependencies, restart/backoff, typed PID handoff | Немає реально запущеного `zillad` або user-space сервісів |
| **PCI / GPU** | Один read-only PCI boot inventory / DeviceRegistry для Storage, VMware SVGA і AMD; моделі DisplayManager | Немає перевіреного AMD/VMware апаратного прискорення, MMIO/DMA/FIFO-активації |
| **Сумісність** | Read-only PE32+ x64 inspection / `pe-inspect` | Немає Win32, DLL loader, Linux ABI чи Android runtime |

**Важливо:** `implemented in source`, `host-tested`, `QEMU-verified`, `VMware-observed` і `hardware-verified` — різні твердження. [STATUS.md](docs/STATUS.md) вказує рівень і джерело доказу для кожної підсистеми.

## Швидкий запуск

### Windows 10/11 + WSL2 Ubuntu + VMware Workstation

Підготуйте WSL2/Ubuntu, PowerShell 5.1+, Rust 1.90.0, потрібні засоби збирання ISO та VMware. Інсталяція залежностей здійснюється тільки явною командою `Setup`.

```powershell
cd M:\projects\ZillaOs
git switch main
git pull --ff-only origin main

# Перевірити передумови
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Check

# Зібрати ISO і запустити VMware
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

Якщо `Check` вказує на відсутні залежності, запустіть `-Action Setup` свідомо. Перед перезавантаженням ISO у VMware зробіть **Power Off**, не Suspend. `-Action Run` відкриває **наявний** ISO без нової збірки. Деталі: [Windows Builder](docs/WINDOWS_ISO.md).

### Linux / WSL2: збірка, QEMU, регресії

```bash
# У корені репозиторію
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
python3 -m unittest discover -s tests -v

# Підготувати демонстраційний ISO (потрібні Limine/xorriso)
bash scripts/build.sh demo

# Інтерактивний QEMU, якщо QEMU/OVMF установлені
python3 scripts/run_vm.py --display gtk
```

Повні сценарії [Build/Development](docs/DEVELOPMENT.md), [QEMU/VMware](docs/BUILD.md), [матриця доказів](docs/verification/EVIDENCE_MATRIX.md). Запуск команди у README не означає, що вона виконувалась на вашій фізичній машині.

## Архітектурні шари

```text
  Embedded Desktop / Start / Files / Notepad (сьогодні)
               |                |
    ApplicationManager      WindowManager
               |                |
     built-in Rust views     software framebuffer
               \________________/
                        |
               Rust no_std kernel
        memory • paging • traps • Platform PCI
                        |
                 Limine / UEFI

  Наступні ізольовані runtime (не активовані як звичайний Desktop):
    ProcessManager ↔ IPC/capabilities ↔ ServiceManager
           |                   |               |
      user programs        resource handles    services
           +------ DisplayManager / device contracts
```

Тримаймо **одне джерело стану**: процеси — ProcessManager, вікна — WindowManager, пристрої — DeviceRegistry, режим виводу — DisplayManager, запуск застосунків — ApplicationManager. Подробиці: [поточна архітектура](docs/ARCHITECTURE_CURRENT.md) та [власники модулів](docs/AGENT_COORDINATION.md).

## Як розвиваємо ZillaOS

Розробку організовано у **шість окремих потоків**: Platform/PCI, Process/Scheduler, IPC/Capabilities, Services, Graphics/VMware та AMD. Кожен агент створює **нову гілку від поточного `main`**, пише тести та PR. Спільні файли інтегрує Chief Architect. Старі I-M1 гілки вже прибрані; починати з них не можна.

Для нетривіальної зміни застосовуємо **spec-first**: перевіряємо факти в репозиторії → визначаємо поведінку і межі → фіксуємо специфікацію → додаємо негативні тести → реалізуємо → підтверджуємо CI. Цей процес адаптовано до ZillaOS із [нашої версії Spec-First Bootstrap](https://github.com/kabasik007/spec-first-bootstrap), побудованої на ідеях [Potapenko](https://github.com/potapenko/spec-first-bootstrap). Це **методологія**, а не залежність ядра.

- [Правила агентів](AGENTS.md) · [Mission Control I-M2](https://github.com/kabasik007/ZillaOs/issues/25) · [Старт нових чатів](docs/NEW_CHAT_START_I_M2.md)
- [Специфікації](docs/specs/README.md) · [Документаційна політика](docs/DOCUMENTATION_POLICY.md) · [Стратегія Bootstrap](docs/BOOTSTRAP_ADOPTION.md)
- [Поточний статус](docs/STATUS.md) · [Roadmap](docs/ROADMAP.md) · [Безпека](docs/SECURITY_ARCHITECTURE.md)

## Карта репозиторію

```text
boot/                  Limine / UEFI конфігурація
src/main.rs            kernel startup і opt-in experiment entrypoints
src/lib.rs             host-testable no_std library
src/kernel/            memory, paging, exceptions, process, IPC, caps
src/platform/          PCI identity, inventory, DeviceRegistry
src/drivers/gpu/       AMD read-only diagnostics, VMware SVGA models
src/video/             DisplayManager і surface/output contracts
src/shell/  src/apps/  Desktop, WindowManager, built-in apps
src/services/          AppManager, ProcessManager gateway, Service models
src/fs/                RAMFS і read-only storage/installer models
scripts/               ISO/Windows/WSL/QEMU build та smoke utilities
tests/                 host integration / negative regressions
docs/                  документація, specs, рішення та verification evidence
.github/workflows/     автоматичні CI gates
```

## Ліцензія та обмеження

ZillaOS поки є дослідницьким проєктом. У корені репозиторію **немає окремого `LICENSE` файла**, тому не слід оголошувати його ліцензованим під MIT/GPL без рішення власника. Не запускайте експериментальні операції з GPU, PCI, firmware чи дисками на основному обладнанні без окремого аудиту та резервного копіювання.
