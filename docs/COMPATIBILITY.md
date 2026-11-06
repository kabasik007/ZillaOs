# ZillaOS — від власного ядра до багатоплатформної ОС

## Що використовується від Linux, а що власне

ZillaOS — незалежна експериментальна ОС із **власним Rust no_std
x86-64 ядром**, а не дистрибутив Linux. Початковий framebuffer,
карту пам'яті та завантаження ELF передає Limine через UEFI.
Після завантаження в VMware у ZillaOS **немає Linux kernel,
systemd, Win32, Android Runtime або POSIX-середовища**.

WSL2 Ubuntu потрібна **на машині розробника** для Cargo, Rust,
xorriso та тестів. Вона не є частиною ZillaOS ISO.

**Реально працювало у VMware:** завантаження ISO, PS/2 миша,
вікна, Files, перегляд read-only папок/файлів. Новий код:
секундний RTC-годинник, висувне меню Start, рухомі значки папок;
ці зміни пройшли host-CI, але потребують окремої перевірки GUI
в VMware після оновлення ISO.

## Високорівневі шари

```text
+-----------------------------------------------------+
| Zilla Desktop / Files / Settings / Terminal         |
+-----------------------------------------------------+
| App Center / Installer / Zilla Native App SDK       |  future
+-----------------------------------------------------+
| Runtime Broker                                      |  future
|  | native ELF | Linux ABI | Win32 | Android | games  |
+-----------------------------------------------------+
| Processes + ring3 + IPC + syscalls + VFS            |  future
+-----------------------------------------------------+
| ZillaOS Rust kernel: memory / paging / IDT / boot   |
+-----------------------------------------------------+
| Device drivers: PS/2, CMOS RTC, framebuffer         |
| Future: graphics / storage / network / audio        |
+-----------------------------------------------------+
| x86-64 + UEFI + Limine                              |
+-----------------------------------------------------+
```

Кожна чужа програма запускатиметься лише **в ізольованому процесі**
або гостьовій VM, ніколи не як невідомий код у ядрі.

## Окремі шляхи сумісності

### ZillaOS Native
Майбутній ELF loader, ізоляція віртуальної пам'яті, системні
виклики й власний ABI. У нас поки немає user-space loader.

### Linux
ELF — тільки формат файлу; потрібна Linux ABI (syscalls,
динамічні бібліотеки, signals, threads, VFS) або VM із Linux.
Просте розпізнавання ELF не запускає Linux-програми.

### Windows
Вже реалізовано сувору перевірку PE32+ x86-64 заголовків та
окрему host-утиліту `pe-inspect`, що використовує той самий Rust-парсер.
**Виконання EXE досі недоступне.** Наступний крок — ізольований PE
image mapper, Win32/NT API й DLL-сумісність або Windows VM.
Wine — приклад сумісного шару Win32, що перекладає виклики
в POSIX API. Без POSIX або адаптації Wine до власного API
напряму скопіювати його в ZillaOS неможливо.
Proton також використовує Wine та інші компоненти й призначений
для ігор Windows у Linux.

### Android
APK — пакет; для запуску потрібні Android Runtime / Binder,
framework services, Android filesystem, графічні драйвери,
JNI та, за потреби, ARM-емуляція або Android VM.
Waydroid працює через Linux namespaces та Binder;
нинішня ZillaOS не підтримує цей механізм.

### PlayStation та Xbox
Кожне покоління — окрема ціль емуляції: CPU/ISA, GPU,
аудіо, syscalls, контент, контролери, модель пам'яті.
RPCS3 — приклад відкритого емулятора саме PlayStation 3,
а не всіх PlayStation. Ми не включаємо чужі закриті прошивки,
ліцензійні ключі та засоби обходу захисту в ZillaOS.

## Чесний реєстр сумісності

У `src/compat/mod.rs` створено базову архітектуру для
визначення ELF / PE / ZIP сигнатур і переліку потенційних
`TargetPlatform`. **Усі runtime позначені як NotImplemented.**
ZIP не обов'язково APK, ELF не обов'язково Linux ABI, а PE
не є дозволом на виконання неперевіреного коду.

Майбутній пакетний маніфест матиме `name`, `version`,
`target_os`, `arch`, `entrypoint`, `permissions`,
`signature`, `runtime_version`. Застосунки завантажуються
тільки через ізольований App Manager.

Практичний стан та команди перевірки Windows-файлів описані в
[WINDOWS_GRAPHICS.md](WINDOWS_GRAPHICS.md).

## Практичні етапи

1. Поліпшити UI, систему файлів RAMFS із контрольованим записом,
   текстовий редактор, налаштування та часові пояси.
2. Кільце 3, ELF loader, адресні простори, scheduler, syscall
   write/exit: перша власна програма поза ядром.
3. VFS, read-only block storage у VM, пізніше журналюваний запис.
4. Native SDK + App Manager + sandbox/capabilities.
5. Linux ELF/sycall MVP для простих Linux-додатків.
6. Windows API/VM, Android runtime/VM як окремі модулі.
7. GPU, аудіо й контролери, потім емулятори конкретних консолей.

**Критерій готовності:** запущений тестовий застосунок у VMware
і окремі автоматичні перевірки; не лише ім'я папки чи сигнатура файлу.

## Технологічні референси

- Wine: https://www.winehq.org/about
- Valve Proton: https://github.com/ValveSoftware/Proton
- Waydroid: https://github.com/waydroid/waydroid
- RPCS3: https://github.com/RPCS3/rpcs3
