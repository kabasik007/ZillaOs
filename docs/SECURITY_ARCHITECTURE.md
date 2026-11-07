# ZillaOS — Security-First Architecture & Threat Model

> **SECURITY SCOPE NOTE (2026-11-07):** This is a threat model, not certification. Legacy two-agent references/absence of all IPC models are historical; see [six current owners](AGENT_COORDINATION.md) and [STATUS](STATUS.md) for the present models and missing runtime gates.

**Статус:** проєктна політика й acceptance gates, **не** сертифікація
безпеки. Станом на 2026-11-06 реальна ізоляція Ring 3, IPC,
PermissionManager, Task Manager і SecurityMonitor ще не реалізовані.

## 1. Мета і модель загроз

ZillaOS має прагнути до **least privilege, default deny, process
isolation, explicit capabilities та auditability** для власних
застосунків і майбутніх Linux/Windows/Android runtime. Обіцяти
«ОС без вірусів» або невидимість malware без доказів не можна.

Вважаємо **недовіреними**: імпортовані ELF/PE/APK, документи,
сторонні драйвери, мережеві пакети, скрипти, файлові шляхи, графічні
команди застосунків, IPC payload, незнайомі DLL та ROM/прошивки.
Навіть «підписано» не означає «безпечний код».

Можливі загрози:
- escalation у Ring 0 через syscall, драйвер чи GPU BAR/MMIO;
- читання/перезапис чужої пам'яті, домашніх документів і секретів;
- ransomware-подібні масові файлові записи; захоплення дозволів;
- persistence/autostart без явної згоди;
- приховування активності, spoofing PID/підписів і логів;
- DoS: процеси/пам'ять/FIFO/CPU, рекурсивні запускання;
- supply-chain: підміна пакетів, boot artifacts, залежностей;
- конфіденційність: небажані мережеві запити й телеметрія;
- скомпрометований kernel/firmware: поза довірою суто UI-аудиту.

## 2. Непорушні інваріанти

1. **Ніякого виконання невідомого коду в Ring 0.**
   `ApplicationManager` відкриває лише зареєстровані вбудовані view,
   `ForeignBinary` не виконується. Після native loader — user mode,
   окремий адресний простір і перевірка пам'яті.
2. **Один реєстр процесів** у ProcessManager (PID/PPID, власник,
   executable origin, стан, ліміти, exit/fault). Створення та
   завершення можливі тільки через контрольований інтерфейс ядра.
3. **Kernel-enforced permissions**: кожна системна дія перевіряє
   права/handle на момент використання, не лише при запуску.
   UI/Task Manager показує рішення, але не є security boundary.
4. **W^X/NX, U/S isolation, guarded stacks, checked copy_from_user**
   перед першим real Ring 3. План прав сторінок у тестах не замінює
   фактично встановлені PTE.
5. **Default deny** для непереданих явно ресурсів. Не можна
   використовувати глобальні ambient-дозволи у сторонніх runtime.
6. **No unsafe disk writes by default** у develop-VM: спочатку
   disposable image, перевірка парсера, read-only mount, потім
   явне opt-in на запис. Не торкатися дисків Windows-хоста.
7. **Bounded parsing/resource caps**: перевірка переповнень, RVA,
   pointer ranges, розміру повідомлень, FIFO та квот на процеси.
8. **Надійний журнал**: одне джерело kernel events, захист від
   переповнення, явні drop counters, часова прив'язка, відсутність
   секретів у логах. RAM-only лог **не є forensic-proof**.

## 3. Цільові служби (це ще не існуючі демони)

| Сервіс | Відповідальність | Важлива межа |
| --- | --- | --- |
| `ProcessManager` | PID, lifecycle, parent-child, crash/fault, квоти | Kernel має перевіряти фактичний стан |
| `PermissionManager` | grant/revoke capabilities, per-resource policies | Усі IO/syscall шляхи проходять перевірку |
| `SecurityMonitor` / `zillaguard` | спостереження, правила, alerts | Не має bypass доступу до всіх файлів |
| `ServiceManager` / `zillad` | запуск і restart дозволених служб | Signed/allowlisted manifests; без root-all |
| `TaskManager` | прозорість процесів, IO, доступи, kill | UI не може приховати системні записи |
| `EventLog` | структурована історія рішень | Ліміт, dropped-events counter, integrity |

**Ніколи не створювати два ProcessManager:** окремий GUI-реєстр і
окремий системний процесний реєстр. Task Manager має читати
складений **read-only snapshot** з офіційного ProcessManager;
«вікна в Dock» не дорівнюють PID.

## 4. Зразок майбутньої моделі дозволів

```text
App manifest (identity, arch, signer, declared capabilities)
            |
  Verified launch + policy (user/admin rules)
            |
 ProcessManager: address space + resource handles
            |
 Syscall / VFS / network / GPU / IPC boundary
       |               |
    authorized       denied + audited
```

Ресурси: `fs.read:/Users/Guest/Documents/<chosen>`,
`fs.write:<chosen>`, `network.connect:<destination>`,
`clipboard.read`, `clipboard.write`, `audio.capture`,
`camera.capture`, `gpu.surface`, `process.spawn`,
`autostart`, `system.manage`.

Це **запропоновані назви**, не існуючий ABI. Зберігати дозволи за
захищеними ідентичностями ресурсів, а не довіряти рядкам шляху або
user-supplied PID. Розділити «попросити» і «отримати» право. Для
підвищених дій потрібне явне схвалення. Grant/revoke перевіряти
під час операції, а не лише на старті.

## 5. Порядок реалізації (Security milestones)

**S0 — тепер:** метадані PID, policy models, fail-closed чужі EXE,
read-only PCI/GPU probe, bounded RAMFS; не рекламуємо це як sandbox.
Докладніше: `docs/PROCESSES.md`, `docs/VMWARE_SVGA_DRIVER.md`.

**S1 — CPU isolation:** 3-рівнева привілеєва межа x86 Ring 3,
CR3/page-table ownership, GDT/TSS, user stack guard, safe traps,
user-fault containment. Тест: user code не може читати kernel
mapped memory; fault закінчує тільки user task.

**S2 — native processes:** kernel-owned PID/PPID lifecycle,
controlled syscall write/exit, scheduler, teardown, quotas.
Тест: дві незалежні програми, конкурентні ресурси, завершення однієї
не руйнує іншу.

**S3 — permissions:** capability tables з непідроблюваними handles,
служби доступу до VFS/IPC/GPU, revoke tests, deny by default.
Тест: програма не може читати/змінювати чужі файли й отримувати
несанкціонований доступ до пристроїв.

**S4 — visibility:** Task Manager з реальними PID, process tree,
CPU/memory counters тільки коли їх вимірюють; SecurityMonitor з
структурованими event records і дроп-контрольованим ring buffer.
Тест: кожен kernel spawn/exit/fault створює audit event;
невідомий PID не може підставити чужий запис.

**S5 — foreign runtimes:** PE DLL/Win32, Linux ABI, Android;
права та логи **ті самі**, що в native-програм, незалежно від формату.
Тест: контрольований чужий executable не може обійти policy через
додатковий runtime.

**S6 — trust chain:** перевірений boot і підписи пакетів,
оновлення та recoverability; детальний threat model firmware
і host hypervisor. Цей етап окремий від sandbox.

## 6. Гейти тестування

- Parser fuzz/property/negative tests для PE/ELF/архівів/протоколів.
- Fault injection, guard pages, integer-overflow і invalid-handle tests.
- Перевірені CPU access rights у VMware/QEMU, **не** тільки unit tests.
- Fork/kill/restart/service failure із доведеним відновленням.
- Перевірка GPU DMA/MMIO bounds та sandbox compositor inputs.
- Тести на відмову при переповненні журналу, квоти, revocation race.
- Відтворюваний boot image hash і provenance для build artifacts.
- Публікувати **факт, тест, платформу, дату**; не заявляти
  «antivirus-proof», «100% safe», «неможливо приховати процеси».

## 7. Робота двох активних агентів

**Windows/Process agent** володіє S1–S3, Runtime/PE, контрактами
ProcessManager; не реалізує паралельний Task Manager registry.

**Graphics/SVGA agent** володіє BAR/FIFO/DisplayManager і майбутнім
compositor; не отримує права виконувати чужі binary/працювати з
процесними PTE довільно. Власний GPU surface доступ має проходити
через майбутні capabilities.

Система безпеки має об'єднувати результати двох напрямів, **не
дублювати** їх. Правила спільних змін: [AGENT_COORDINATION.md](AGENT_COORDINATION.md).

Див. також [ARCHITECTURE_CURRENT.md](ARCHITECTURE_CURRENT.md),
[ARCHITECTURE.md](ARCHITECTURE.md) і [DECISIONS.md](DECISIONS.md).
