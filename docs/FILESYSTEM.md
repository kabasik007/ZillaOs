# ZillaOS — гостьова файлова ієрархія та модулі вихідного коду

## 1. Папки всередині ZillaOS (не на диску Windows)

Це перший прототип файлової системи з **незмінною структурою,
скомпільованою в ядро**, і окремим обмеженим записуваним
RAMFS-шаром для текстових файлів у Documents. Вона доступна для перегляду у Files, працює
в RAM і **не читає та не записує диски** VMware чи хоста Windows.

```text
Z:/                                    ZillaOS virtual root
├── Program Files/
│   ├── Zilla Files/
│   ├── Zilla Terminal/
│   ├── Zilla Settings/
│   ├── Zilla App Center/
│   └── Zilla Notepad/
├── Users/
│   ├── Guest/
│   │   ├── Desktop/
│   │   ├── Documents/
│   │   │   ├── WELCOME.TXT (read-only built-in)
│   │   │   └── NOTE01.TXT (після першого Save, volatile)
│   │   ├── Downloads/
│   │   ├── Pictures/
│   │   ├── Music/
│   │   └── Videos/
│   └── Public/
│       └── Documents/
├── System/
│   ├── Drivers/
│   ├── Fonts/
│   ├── Config/
│   ├── Logs/
│   └── ABOUT.TXT
└── Temp/
    └── Cache/
```

Усередині коду канонічні абсолютні шляхи мають вигляд
`/Users/Guest/Documents`. У Files показуємо назви папок без
небезпечного доступу до `C:\` чи `M:\` Windows. Назви каталогів
стабільні незалежно від мови GUI.

**Files:** одинарний клік відкриває папку або текстовий перегляд,
`Backspace` повертає до батьківського каталогу (або закриває preview).
Пункт HOME у лівій панелі повертає в корінь. Вміст
`WELCOME.TXT` і `ABOUT.TXT` — текстові приклади, вбудовані в ядро.

**Безпека:** відносні шляхи, `..`, зайві роздільники та невідомі
імена не приймаються. Відсутній запис на реальні пристрої. У RAMFS
дозволено створювати й перезаписувати тільки текстові `.TXT` файли
з допустимими ASCII-іменами, максимум 8 файлів по 512 байтів.
Наразі немає видалення, переміщення та постійного зберігання.
Це не NTFS і поки не FAT32-драйвер.

**Zilla Notepad:** Save записує у `/Users/Guest/Documents` в
оперативній пам'яті; файли можна відкрити через Files. Деталі в
[NOTEPAD.md](NOTEPAD.md).

## 2. Організація репозиторію (структура вихідного коду)

Усі основні файли поступово переносимо в окремі підсистеми.
Це реальна структура `main` після перенесення, а не лише план:

```text
ZillaOs/
├── boot/                    Limine + UEFI конфігурація
├── scripts/                 Windows/WSL builder, ISO, VMware
├── src/
│   ├── main.rs              ранній запуск ядра, RTC і GUI event loop
│   ├── lib.rs               no_std API (стабільні назви модулів)
│   ├── kernel/
│   │   ├── memory.rs
│   │   ├── paging.rs
│   │   └── exceptions.rs
│   ├── drivers/
│   │   ├── mod.rs           драйвери раннього завантаження
│   │   ├── rtc.rs           CMOS RTC, read-only
│   │   ├── mouse.rs         PS/2 packet decoder
│   │   └── storage_pci.rs
│   ├── fs/
│   │   ├── mod.rs           read-only базове дерево
│   │   ├── ramfs.rs         volatile .TXT файли
│   │   ├── gpt.rs
│   │   └── install_plan.rs
│   ├── ui/
│   │   └── raster.rs        RGB32, clip/region/backbuffer алгоритми
│   ├── shell/
│   │   ├── desktop.rs       Desktop state, input-driven events
│   │   ├── window_manager.rs
│   │   ├── locale.rs        UI переклади UA / EN / RU
│   │   ├── clock.rs         перевірка RTC-даних
│   │   ├── desktop_icons.rs рухомі ярлики каталогів
│   │   ├── start_menu.rs    геометрія слайд-анімації
│   │   ├── files_interaction.rs
│   │   └── mod.rs
│   ├── video/
│   │   └── mod.rs           PCI graphics discovery (read-only)
│   ├── compat/
│   │   └── windows_pe.rs    Windows PE32+ x64 inspector
│   ├── bin/
│   │   └── pe_inspect.rs    host CLI (not in boot ISO)
│   ├── apps/
│   │   ├── mod.rs           каталог вбудованих програм
│   │   ├── notepad.rs       текстовий редактор
│   │   └── notepad_ui.rs    спільні координати кнопок
│   ├── compat/
│   │   └── mod.rs           ELF/PE/ZIP detector, backends disabled
│   ├── setup.rs             майстер першого запуску
│   ├── graphics.rs          bootstrap renderer (план: ui/)
│   ├── input.rs             polling PS/2 (план: drivers/)
│   └── ...                  решта boot-platform модулів
├── tests/                   host-тести та захист boot-артефактів
└── docs/
```

Старі публічні Rust-модулі (`crate::desktop`, `crate::memory`,
`crate::gpt` тощо) доступні під колишніми іменами через явні
`#[path = "..."]` у `lib.rs`. Це дозволяє переносити вихідні файли,
не змінюючи весь код одночасно.

У Desktop додано **4 рухомі ярлики**:
Documents, Pictures, Downloads, Program Files.
Клік відкриває відповідний віртуальний каталог у Files;
перетягування змінює лише положення значка у RAM.
Ці координати та папки поки не зберігаються після вимкнення VM.

## 3. Наступні реальні кроки

1. **VFS API:** `open/read/list/create/remove`, типи прав і
   розмежування mount points. Спочатку `ramfs`, без дисків.
2. **RAM-backed файли:** створення й редагування невеликих файлів
   із лімітами обсягу та повідомленням про відсутність збереження
   після перезавантаження.
3. **Профілі користувачів:** поточний `Guest` — лише назва папки;
   потрібно розробити ідентичність, права, домашній каталог,
   окремі адресні простори та процеси.
4. **Застосунки:** розділити вбудовані демо-вікна на процеси,
   визначити власний формат пакета/маніфесту, тільки потім
   установлення в `Program Files`.
5. **Накопичувачі:** read-only драйвер блочного пристрою, FAT32
   читання, тести пошкоджених образів; запис дозволяти лише
   після окремого аудиту та відпрацювання на disposable image.
6. **Збереження параметрів:** системні налаштування, мова, тема,
   персональні файли; лише з перевіреною файловою системою.

На цьому етапі розділення каталогів не означає сумісності
з Windows `.exe` або її `Program Files`. Це **власна модель ZillaOS**.
