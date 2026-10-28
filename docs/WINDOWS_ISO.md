# ZillaOS — Windows: ISO за декілька кліків

**Стан: 28 жовтня 2026 року.** Усі Windows-скрипти зберігаються в
репозиторії, але збірка Windows/WSL/VMware **не була фактично запущена**
в середовищі асистента. Після першої збірки треба перевірити результат
і COM1-журнал. Це експериментальна система з графічним Setup і Desktop,
**не готовий інсталятор на фізичний диск**.

## Чим створюємо ISO?

- **Rust 1.90.0** компілює ядро ZillaOS для `x86_64-unknown-none`.
- **Limine 12.9.4** завантажує ELF-ядро у режимі UEFI.
- **xorriso** збирає ISO9660 з EFI El Torito-завантаженням.
- **WSL2 Ubuntu** запускає ці інструменти просто з Windows.
- **VMware Workstation** відкриває готовий ISO в ізольованій VM.

**Rufus не потрібний** для створення ISO: це переважно утиліта для
запису готових образів на USB. UltraISO також не потрібний для збірки
Rust-ядра й UEFI-завантажувача.

## 1. Один раз встанови WSL2 + Ubuntu

У Windows PowerShell **від адміністратора**:

```powershell
wsl --install -d Ubuntu
```

Якщо Windows запропонує — перезавантаж ПК. Відкрий **Ubuntu** через
меню «Пуск» та створи Linux-користувача і пароль. Після цього всі
наступні команди запускаються у звичайній PowerShell або через меню,
**без адміністратора** (Ubuntu може попросити `sudo`-пароль).

Перевір ім'я дистрибутива:

```powershell
wsl --list --verbose
```

Якщо він називається, наприклад, `Ubuntu-24.04`, у PowerShell-скрипті
використовуй `-Distro Ubuntu-24.04`. CMD-меню за замовчуванням
розраховане на дистрибутив із назвою `Ubuntu`.

## 2. Онови ZillaOS і відкрий Windows-меню

Проєкт у Windows:

```text
M:\projects\ZillaOs\
    ZillaOS-Windows.cmd
    scripts\windows.ps1
    scripts\windows_wsl_setup.sh
    scripts\build_vmware.sh
    scripts\build.sh
    scripts\vmware.ps1
```

Отримай останні зміни (`git pull origin main`) у папці репозиторію,
а потім **двічі натисни `ZillaOS-Windows.cmd`**.

В меню:
- **1 — Doctor:** перевірка Ubuntu, Rust, `xorriso` і залежностей,
  без встановлення програм.
- **2 — Setup:** встановлення залежностей **лише в Ubuntu** та
  Rust 1.90.0 за твоїм вибором. Може попросити sudo-пароль і мережу.
- **3 — Build ISO:** запуск тестів, компіляція ядра та створення UEFI ISO.
- **4 — Build + VMware:** збірка ISO й відкриття тестової VM.
- **5 — VMware:** відкрити вже створений ISO, не збираючи знову.
- **6 — Explorer:** відкрити папку `dist` із готовим ISO.
- **7 — QEMU:** графічний тест, якщо встановлено QEMU/OVMF.
- **8 — Setup + QEMU:** установити також QEMU, OVMF та mtools у WSL.

**Setup — явна дія:** пункти збірки й запуску не встановлюють
залежностей приховано.

## 3. Один рядок без меню (Windows PowerShell)

Відкрий Windows PowerShell у папці `M:\projects\ZillaOs`:

```powershell
# Одноразово: встановити xorriso, Python, Rust/target у WSL Ubuntu:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Setup

# Лише перевірити стан:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Doctor

# Створити готовий UEFI ISO:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Build

# Зібрати й відкрити VMware Workstation:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

Коли WSL уже налаштовано, для повторної збірки зазвичай потрібен лише
`-Action Build` або пункт **3** у CMD-меню.

### Що створюється

```text
M:\projects\ZillaOs\dist\
    zillaos-0.0.2-dev-demo-uefi.iso    (оригінал збірки)
    ZillaOS-Desktop-UEFI.iso           (зрозуміла назва для Windows)
    zillaos-0.0.2-dev-demo-kernel.elf
```

Windows-обгортка копіює ISO під зручною назвою лише **після успішної
збірки** і перевіряє, що SHA-256 двох ISO збігаються. Лог збірки:
`build\logs\windows-iso-build.log`.

Сам ISO створюється цим уже наявним викликом:

```bash
xorriso -as mkisofs -R -r -J \
  --efi-boot boot/limine-uefi-cd.bin \
  -efi-boot-part --efi-boot-image \
  -o dist/zillaos-0.0.2-dev-demo-uefi.iso build/iso-root
```

Перед тим `scripts/build.sh` завантажує **закріплену версію Limine**,
перевіряє SHA-256 її архіву, збирає Rust-ядро й готує `build/iso-root`.
Простий запуск `xorriso` без цієї попередньої підготовки не створить
коректного ISO ZillaOS.

## 4. Запуск у VMware

CMD-меню **4** (або PowerShell `-Action BuildRun`) запускає
наявний `scripts/vmware.ps1`:

- UEFI, Secure Boot вимкнено;
- 1 vCPU, 1 ГіБ RAM, тестова SVGA;
- лише віртуальний CD/DVD із ZillaOS ISO;
- **немає диска, мережі, USB passthrough чи спільних папок**;
- COM1-журнал: `build\vmware\ZillaOS\zillaos-com1.log`.

У VMware треба натиснути **Power on**. Після завантаження має
показуватися дев'ятиетапний Zilla Setup і демонстраційний Desktop.
Це очікувана поведінка, **не перевірена поки фактичним VMware boot**.

У PowerShell можна подивитися COM1:

```powershell
Get-Content .\build\vmware\ZillaOS\zillaos-com1.log -Tail 100
```

## 5. Типові проблеми

| Помилка | Що робити |
| --- | --- |
| `wsl.exe` не знаходить Ubuntu | `wsl --list --verbose`; встановити/завершити реєстрацію Ubuntu |
| `Missing cargo/rustc` | CMD-меню **2** або PowerShell `-Action Setup` |
| `Missing xorriso` | CMD-меню **2** |
| `Rust toolchain 1.90.0 missing` | CMD-меню **2**, інструмент встановить закріплену версію |
| `Limine SHA-256 mismatch` | Не вимикати перевірку; перевірити кеш і перевантажити архів |
| `cargo build` видає помилки | Надіслати повний `windows-iso-build.log`; ISO може ще не існувати |
| ISO є, VMware чорний | Перевірити UEFI, вимкнений Secure Boot, COM1 і `vmware.log` |
| Помилка `Permission denied` | Переконатися, що репозиторій доступний WSL на `/mnt/m/projects/ZillaOs` |

Для більш суворого тестування ISO/QEMU див.
[BOOT_DIAGNOSTICS.md](BOOT_DIAGNOSTICS.md) і
[INSTALLER_ACCEPTANCE.md](INSTALLER_ACCEPTANCE.md).

**Безпека:** ISO використовується лише у віртуальній машині.
Не записуй експериментальну ZillaOS на фізичні диски та не вмикай
доступ до host-дисків у VMware. У системі поки немає готових драйверів
диска чи постійного збереження налаштувань.
