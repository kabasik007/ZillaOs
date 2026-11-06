# ZillaOS Application Framework — v0 (2026-11-06)

## Мета

Створювати незалежні застосунки ZillaOS, не дублюючи логіку у
Start Menu, Dock, ярликах, файловому менеджері та майбутньому App Store.

Поточна реалізація — **синхронні модулі раннього Rust-ядра**. Вона
не створює користувацькі процеси, не завантажує ELF/PE/APK, не запускає
сторонній код і не має повноцінного IPC. Ці можливості не можна
вважати реалізованими лише тому, що є модуль із назвою "services".

## Поточний потік запуску

```text
F10 / Start / Dock / Desktop Icon / App Center
                   |
              App ID / path
                   |
       services::app_manager
           ApplicationManager
                   |
           apps::BUILTIN_APPS
              (registry)
                   |
         shell::window_manager
       (one owner of window state)
                   |
        graphics (embedded view)
```

Вікна та їх стани (відкрито, згорнуто, фокус, координати)
зберігаються **тільки** в `WindowManager`. App Manager не веде
другого списку «запущених програм». Він перевіряє ідентичність
встановленого застосунку та передає команду існуючому менеджеру вікон.

**Список вбудованих програм — один:**

| App ID | Каталог ZillaOS | UI |
| --- | --- | --- |
| Files | `/Program Files/Zilla Files` | файловий переглядач RAMFS |
| Terminal | `/Program Files/Zilla Terminal` | демонстраційне вікно, не shell |
| Settings | `/Program Files/Zilla Settings` | поточні volatile налаштування |
| AppCenter | `/Program Files/Zilla App Center` | каталог і відкриття вбудованих застосунків |
| Notepad | `/Program Files/Zilla Notepad` | редактор `.TXT` із записом у RAMFS |

`src/apps/id.rs` — стабільні App ID;
`src/apps/mod.rs` — дескриптори, шляхи, іконки й локалізаційні ключі;
`src/apps/app_center.rs` — геометрія кліків для App Center;
`src/apps/notepad.rs` — модель текстового редактора;
`src/apps/notepad_ui.rs` — панель NEW/SAVE/DOCUMENTS;
`src/fs/ramfs.rs` — обмежений volatile запис `.TXT`;
`src/services/app_manager.rs` — єдина точка запуску;
`src/shell/window_manager.rs` — фокус, вікна, мінімізація й переміщення;
`src/shell/start_menu.rs` — лише геометрія Start;
`src/graphics.rs` — поточний вбудований renderer;
`src/compat/mod.rs` — наразі всі foreign runtimes вимкнені.

**Робочий Notepad:** редагування ASCII, `F11`/`Ctrl+S` — Save, `F12` — New; збережені документи з'являються в Files → Documents. Подробиці: [NOTEPAD.md](NOTEPAD.md).

## Безпека і межі

- `LaunchTarget::Builtin(App)` і зареєстрований `InstalledPath`
  можуть відкрити/відновити відоме вбудоване вікно.
- `LaunchTarget::ForeignBinary(&[u8])` завжди повертає
  `RuntimeUnavailable`, без парсингу/виконання даних.
- Невідомий шлях повертає `NotInstalled`.
- `Program Files` — лише read-only дерево гостьових каталогів у RAM,
  не Windows `C:\Program Files` і не встановлені .exe.
- Ні меню «Пуск», ні графіка не повинні безпосередньо виконувати
  неперевірений код.

## Додавання нового вбудованого застосунку (етап v0)

1. Створити окремий модуль `src/apps/<name>.rs`, без доступу
   до апаратних I/O портів або довільних адрес.
2. Додати App ID і дескриптор до спільного реєстру.
3. Додати каталог у read-only guest filesystem.
4. Додати view у renderer і slot у WindowManager.
5. Користуватися тим самим ApplicationManager для всіх способів запуску.
6. Написати host-тести моделі, вводу і навігації та перевірити збірку
   `x86_64-unknown-none` у GitHub CI, потім ISO у VMware.

Пізніше view має бути **user-space програмою**, а не включеним у ядро
`match App`. На цьому етапі registry буде передавати запуск
`ProcessService`, а WindowManager прийматиме лише керований surface ID.

## Наступний план впровадження програм

1. **Система вводу і native SDK v0:** `AppManifest`, повноваження,
   події вікна, клавіатури й курсора, ABI версії 1.
2. **RAMFS із контрольованим записом:** `create/read/write/rename/delete`
   у межах RAM і ліміту обсягу; після перезавантаження — очищення.
3. **Zilla Notepad:** перший окремий UX-застосунок з редактором,
   створенням `.txt` і роботою з RAMFS.
4. **ring3 + ELF64 loader:** окремий нативний процес Zilla з мінімальними
   `exit/write` syscalls і ізоляцією пам'яті. Ніяких файлів від
   невідомих джерел у ring0.
5. **Linux compatibility MVP:** статичний x86-64 ELF із чітким піднабором
   Linux syscalls. ELF формат сам по собі не гарантує Linux ABI.
6. **Windows compatibility:** PE loader, Win32/NT API або гостьова VM;
   не можна просто включити Wine в `no_std` ядро.
7. **Android compatibility:** Android Runtime/system services або VM,
   не лише APK ZIP читання.
8. **Console emulators:** окремі рушії для конкретного покоління
   PlayStation/Xbox, після GPU/аудіо/контролерів.

**Критерій «працює»:** застосунок запускається й взаємодіє в VMware
через загальні сервіси, а в перспективі — з ізольованого процесу,
не лише з renderer ядра.

Див. також [план сумісності](COMPATIBILITY.md) і
[структуру каталогів](FILESYSTEM.md).
