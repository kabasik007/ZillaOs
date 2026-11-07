# Власні шпалери ZillaOS — Boot ISO v1

Реальний BMP-фон тепер може бути включений у завантажувальний ISO як
необов'язковий модуль Limine. Це не запис на диск із ядра. Після
перезавантаження із того самого ISO фон знову доступний, а профіль
THEME.TXT у Documents, як і раніше, зберігається лише у RAMFS.

## Як встановити на Windows / VMware

1. Створи папку M:\projects\ZillaOs\assets
2. Збережи своє зображення як M:\projects\ZillaOs\assets\wallpaper.bmp
   у форматі Windows BMP, 24-bit, без стиснення, до 1024×768 пікселів.
3. У PowerShell виконай:

    cd M:\projects\ZillaOs
    git pull --ff-only origin main
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun

Під час збирання скрипт перевіряє приватний BMP, додає його до ISO
і прописує module_path та module_string у всі 6 пунктів Limine.
Вихідний boot/limine.conf залишається незмінним. Якщо файла немає —
звичайна збірка працює без модулів, як і раніше.

Після запуску Desktop автоматично активує власний фон. ПКМ →
Персоналізація → четверта плитка «МОЇ ШПАЛЕРИ». Без валідного
модуля плитка показує N/A і не змінює оформлення.

## PNG/JPEG

Для необов'язкового імпорту PNG/JPEG у WSL Ubuntu (Pillow необхідний
лише для конвертації; у kernel і стандартному BuildRun його немає):

    python3 -m pip install pillow
    python3 scripts/wallpaper_asset.py import /path/to/photo.jpg --output assets/wallpaper.bmp

Скрипт пропорційно зменшує великі зображення до 1024×768 і зберігає
24-bit BMP. Готовий BMP можна перевірити без збірки:

    python3 scripts/wallpaper_asset.py validate assets/wallpaper.bmp

## Безпека та обмеження

- BITMAPINFOHEADER 40 байтів, BI_RGB, 24/32 bpp, top-down/bottom-up.
- Максимум 1024×768, 4 MiB файла, 3 MiB RGB32-пікселів у BSS.
- Перевіряються всі offsets, strides, boundaries, dimensions і headers.
- Limine bytes копіюються в пам'ять ядра до будь-якого reclaim.
- Масштабування nearest neighbour до поточного GOP mode; без GPU прискорення.
- Якщо модуль відсутній або неправильно сформований — безпечний fallback.
- Приватний assets/wallpaper.bmp ігнорується Git, тому фото не потрапляє
  в GitHub через звичайний git add.
- Заміна файла вимагає нового ISO і перезапуску VM.
- Запис THEME.TXT на SSD, імпорт із Files і PNG/JPEG у ядрі ще відсутні.

COM1 маркери: ZILLAOS_WALLPAPER_MODULE=READY (скопійовано і перевірено)
або ZILLAOS_WALLPAPER_MODULE=ABSENT_OR_INVALID (процедурний fallback).

## Перевірка

    python3 -m unittest discover -s tests -v
    cargo test --lib --target x86_64-unknown-linux-gnu --offline
    bash scripts/build.sh demo
    python3 scripts/desktop_smoke.py --timeout 120

**Автоматична QEMU-перевірка від 2026-11-07 — успішно:**

- 254 Rust unit tests: passed, 0 failed.
- Стандартний ISO без шпалер: Desktop GUI boot, PS/2 mouse, cursor — passed.
- Синтетичний 8×8 magenta BMP вкладається в ISO як Limine module,
  а COM1 підтверджує ZILLAOS_WALLPAPER_MODULE=READY.
- QEMU ISO реально відрендерив 309932 пікселі magenta (#FF00FF)
  на скриншоті 1280×800. Це реальна перевірка framebuffer-пікселів,
  а не тільки успішний запуск функції.
- GitHub Actions: https://github.com/kabasik007/ZillaOs/actions/runs/42330941169

**Залишається ручна перевірка у VMware Workstation** із власним файлом
assets/wallpaper.bmp, оскільки автоматизований тест використовує QEMU
та синтетичний BMP, а не реальну фотографію користувача.
