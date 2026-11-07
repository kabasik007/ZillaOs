# ZillaOS documentation · навігатор

**З чого почати:** [README проєкту](../README.md) → [актуальний статус](STATUS.md) → [поточна архітектура](ARCHITECTURE_CURRENT.md). Ця сторінка — **маршрутизатор** між документами, не друга копія README.

## Якщо потрібно…

| Питання | Документ |
| --- | --- |
| Що вже реалізовано й **чим підтверджено**? | [STATUS.md](STATUS.md) · [EVIDENCE_MATRIX.md](verification/EVIDENCE_MATRIX.md) |
| Де канонічні власники стану та шари ядра? | [ARCHITECTURE_CURRENT.md](ARCHITECTURE_CURRENT.md) |
| Як зібрати й запустити на Windows/WSL/QEMU? | [DEVELOPMENT.md](DEVELOPMENT.md) · [BUILD.md](BUILD.md) · [WINDOWS_ISO.md](WINDOWS_ISO.md) |
| Які загрози й апаратні заборони? | [SECURITY_ARCHITECTURE.md](SECURITY_ARCHITECTURE.md) |
| Які рішення були прийняті і чому? | [DECISIONS.md](DECISIONS.md) · [DOCUMENTATION_POLICY.md](DOCUMENTATION_POLICY.md) |
| Що будемо робити наступним? | [NEXT_WAVE_I_M2.md](NEXT_WAVE_I_M2.md) · [GitHub Mission Control #25](https://github.com/kabasik007/ZillaOs/issues/25) |
| Як розподіляємо шістьох агентів? | [AGENTS.md](../AGENTS.md) · [AGENT_COORDINATION.md](AGENT_COORDINATION.md) · [NEW_CHAT_START_I_M2.md](NEW_CHAT_START_I_M2.md) |
| Як описати нову поведінку **перед кодом**? | [Специфікації](specs/README.md) · [Шаблон](specs/templates/feature-spec.md) |
| Чи застосовуємо Potapenko/Bootstrap? | [BOOTSTRAP_ADOPTION.md](BOOTSTRAP_ADOPTION.md) |

## Інженерні документи за темою

| Домен | Головні джерела |
| --- | --- |
| CPU / memory / paging | [MEMORY.md](MEMORY.md) · [PAGING.md](PAGING.md) · [EXCEPTIONS.md](EXCEPTIONS.md) |
| Процеси та життєвий цикл | [PROCESSES.md](PROCESSES.md) · [PROCESS_LIFECYCLE_IM1.md](PROCESS_LIFECYCLE_IM1.md) |
| IPC та права | [IPC_CAPABILITIES_FOUNDATION.md](IPC_CAPABILITIES_FOUNDATION.md) |
| Служби | [SERVICE_MANAGER_MODEL.md](SERVICE_MANAGER_MODEL.md) · [SERVICE_MANAGER_PROCESS_BRIDGE.md](SERVICE_MANAGER_PROCESS_BRIDGE.md) |
| PCI та пристрої | [PLATFORM_IM1_SINGLE_INVENTORY.md](PLATFORM_IM1_SINGLE_INVENTORY.md) · [PCI_DEVICE_FOUNDATION_IMPLEMENTATION.md](PCI_DEVICE_FOUNDATION_IMPLEMENTATION.md) |
| Graphics / GPU | [GRAPHICS_DISPLAY_CONTRACTS.md](GRAPHICS_DISPLAY_CONTRACTS.md) · [VMWARE_SVGA_DRIVER.md](VMWARE_SVGA_DRIVER.md) · [AMD_GPU_A0_IMPLEMENTATION.md](AMD_GPU_A0_IMPLEMENTATION.md) |
| Desktop / застосунки | [DESKTOP.md](DESKTOP.md) · [APPLICATIONS.md](APPLICATIONS.md) · [NOTEPAD.md](NOTEPAD.md) |
| Файли та installer | [FILESYSTEM.md](FILESYSTEM.md) · [INSTALLER_V2.md](INSTALLER_V2.md) · [STORAGE.md](STORAGE.md) |
| Сумісність | [COMPATIBILITY.md](COMPATIBILITY.md) · [PE_LOADER.md](PE_LOADER.md) |

## Як читати старі документи

У `docs/` багато історичних I-M1, M0/M1 і ранніх roadmap файлів. Їх **не видалено**, щоб не втрачати рішення й посилання. Дати й твердження `NOT RUN`, `branch: feat/...` в старих записах відносяться до **тогочасного зрізу**, а не до поточного HEAD.

Пріоритет доказів: поточний код/CI на відомому SHA → [STATUS.md](STATUS.md) → актуальний доменний контракт → історичний план. Принципи оновлення та міграції описано в [DOCUMENTATION_POLICY.md](DOCUMENTATION_POLICY.md).
