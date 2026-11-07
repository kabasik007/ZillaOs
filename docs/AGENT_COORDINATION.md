# Координація паралельних агентів ZillaOS

**Статус:** I-M1 інтегровано в один кандидат; після затвердженого merge починається I-M2 з **шістьма окремими агентами**.  
**Repository:** `kabasik007/ZillaOs` · **baseline:** `main` · **оновлено:** 2026-11-07.  
**Актуальна матриця шести власників:** [`NEXT_WAVE_I_M2.md`](NEXT_WAVE_I_M2.md). Старі два потоки нижче залишені як історичний контекст; у разі конфлікту застосовуй I-M2 правила.

Цей документ — контракт спільної роботи, а не сигнал, що будь-які
функції вже запущені. Перед кожною задачею агенти мають перечитати
актуальні файли HEAD; інший чат може вже мати нові коміти.

## Матриця відповідальності

| Підсистема | Основний власник | Канонічні модулі | Власний документ |
| --- | --- | --- | --- |
| Graphics/SVGA | GPU agent | `src/drivers/gpu/**`, `src/video/**` | `VMWARE_SVGA_DRIVER.md` |
| Process/Windows | Runtime agent | `src/kernel/process/**`, `src/services/process_manager.rs`, `src/compat/**` | `PROCESSES.md` |
| App lifecycle | Integration boundary | `src/services/app_manager.rs`, `src/apps/**` | `APPLICATIONS.md` |
| Window state / GUI | Integration boundary | `src/shell/window_manager.rs`, `src/shell/desktop.rs`, `src/graphics.rs` | `DESKTOP.md` |
| Build/boot | Integration boundary | `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `boot/**`, `scripts/**` | `WINDOWS_ISO.md` |
| README/правила/безпека | Docs agent | `README.md`, `AGENTS.md`, `docs/AGENT_COORDINATION.md`, `docs/ARCHITECTURE_CURRENT.md`, `docs/SECURITY_ARCHITECTURE.md` | ці документи |

Це правило **single-writer per file at a time**, не заборона на сумісну інтеграцію. У новій хвилі **жоден звичайний агент не змінює `main` напряму**: тільки власна гілка та PR від актуального baseline. Chief Architect перевіряє і координує спільні файли. Історична практика прямих записів в `main` скасована.

## Спільні контракти (не дублювати)

1. **WindowManager** — єдиний власник стану вікон, фокуса, геометрії,
   z-order. GPU-композитор отримує лише опис surface/damage, а не
   заводить власну таблицю `App`-вікон.
2. **ApplicationManager** — одна точка маршрутизації запуску з
   Start/Dock/Files/App Center. Сьогодні відкриває тільки вбудовані
   kernel views. Майбутній ProcessManager — окремий backend, не другий
   застосунковий launcher.
3. **ProcessManager** — власник таблиці PID, стану та життєвого циклу
   **справжніх** процесів, коли їх буде реалізовано. Поточна модель
   host-testable; вікно Notepad **ще не процес**.
4. **DisplayManager** (план) — один власник поточного фізичного mode,
   pitch, framebuffer, VRAM aperture, можливості драйвера та rollback.
   Ні Windows Runtime, ні Shell не записують у SVGA MMIO напряму.
5. **Memory/page allocator** — один власник фізичних кадрів,
   прав сторінок, об'єктів відображення та відновлення ресурсів.
   GPU BAR mappings і user process mappings проходять через узгоджений
   kernel memory API, а не через паралельні allocators.
6. **Syscall/IPC ABI** — версіонований контракт. Runtime не викликає
   `graphics.rs`/hardware ports напряму; compositor не інтерпретує
   буфери користувача як довірені.

## Рекомендовані інтеграційні інтерфейси — пропозиція, не реалізація

```text
AppCenter / Files / Start / Dock
              |
        ApplicationManager
              |
       ProcessManager (future ring3)
              |                        +---------------------+
     process IDs / surface handles ---> | WindowManager       |
              |                        +----------+----------+
  Zilla ELF / Windows PE loader                   |
     |          |                                  v
 Native ABI   Win32 shim                      Compositor
     |          |                                  |
  security checks / syscalls              DisplayManager
     |                                         |
   Kernel memory & capabilities            VMware SVGA /
                                           GOP fallback
```

IPC, capability handles і surface grants поки **не створені**.
Така схема визначає цільові межі відповідальності.

## Протокол змін у власних гілках та інтеграції

1. **READ:** отримати поточний commit SHA, файли й релевантні тести.
2. **SCOPE:** записати у звіті, що саме належить своєму потоку і який
   спільний файл зачіпається. Де можливо — додавати окремі модулі.
3. **EDIT:** мінімальний patch поверх щойно прочитаних SHA; не відновлювати
   увесь файл із давно збереженої копії.
4. **VALIDATE:** локально (коли доступно) — unit tests, bare-metal build,
   `fmt --check`, тест синтаксису PowerShell/Python. За необхідності —
   GitHub Actions; тести не дорівнюють VMware runtime verification.
5. **COMPARE:** перед commit/push перечитати HEAD власної feature-гілки. Створювати коміт від поточного батька; власні гілки оновлювати тільки з `expected_sha` або Contents API з актуальним blob SHA. `main` змінює лише Chief Architect після інтеграційної перевірки.
6. **RETRY:** при конфлікті не застосовувати force. Заново читати код,
   переносити лише власний логічний diff, повторно запускати тести.
7. **REPORT:** commit, файли, перевірки, обмеження, наступні кроки.

### Особливо небезпечні зміни

- `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `src/pci_io.rs`:
  обидва агенти можуть потребувати інтеграції. Робити лише маленькі
  взаємно незалежні включення, не переписувати bootstrap або порядок
  ініціалізації без узгодженого контракту.
- `src/shell/desktop.rs`, `src/graphics.rs`: GPU agent змінює
  GUI-інтеграцію; Runtime agent не додає окремий renderer. Коли потрібні
  task/status вікна — спочатку погодити provider API.
- `docs/ARCHITECTURE.md` та `docs/DECISIONS.md`: довгострокові
  проєктні документи, не переписувати паралельно заради косметики.
- Не видаляти `README`, `AGENTS.md`, тести інших потоків чи існуючі
  модулі лише тому, що зміни поки не використовуються.

## Спільні критерії приймання

**GPU:** read-only probe ≠ працездатний mode switch ≠ hardware-accelerated
2D ≠ SVGA3D. Для кожного потрібен окремий тест/лог/VM доказ, безпечний
rollback і GOP fallback.

**Process:** process metadata ≠ kernel scheduler ≠ Ring 3. Потрібні
перевірені user/kern page permissions, exception recovery, справжня
user-mode програма, syscall handler та teardown без kernel panic.

**Windows:** PE parser ≠ PE image mapper ≠ Win32 DLL loading ≠ запуск EXE.
До завершення Ring 3 і sandbox — `ForeignBinary` fail-closed.

**Security:** таблиця задач і UI-індикатор ≠ захист від malware.
Лише примусово застосовувані обмеження в ядрових точках доступу
і відтворювані тести атак можуть бути доказом.

## Уникнення спільного mutable status-файла

Не ведіть один загальний `progress.md`, який двоє агентів постійно
перезаписують. Джерело історії — Git commits і CI, актуальний стан —
модульні документи `PROCESSES.md` і `VMWARE_SVGA_DRIVER.md`. Зведення
у README оновлювати з реальних перевірених фактів, а не з планів.

## Мінімальні команди

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
python3 -m unittest discover -s tests -v
```

Під VMware Windows Builder: `scripts/windows.ps1 -Action BuildRun`
після `git pull --ff-only origin main` і повного **Power Off** VM.

Документи: [AGENTS.md](../AGENTS.md) · [Current architecture](ARCHITECTURE_CURRENT.md) ·
[Security](SECURITY_ARCHITECTURE.md).
