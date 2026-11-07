# ZillaOS — development & verification runbook

**Date:** 2026-11-07 · **owner:** Chief Architect / build workflow.  
**Status:** project workflow for I-M2; commands based on `rust-toolchain.toml`, `Cargo.toml`, `scripts/windows.ps1`, CI. New agents must recheck live `main`.

## Environment and boundaries

- Target runtime: own `#![no_std]` Rust x86-64 kernel, `x86_64-unknown-none`, Limine/UEFI. Do not assume host Linux libraries are available to the kernel.
- Host: Linux/WSL2; pinned Rust **1.90.0**. `xorriso`, QEMU `qemu-system-x86_64`, OVMF for virtual boots; Python 3 for host tools. First bootloader download may require internet.
- Windows: PowerShell 5.1+, WSL2 Ubuntu, VMware Workstation for local GUI testing. `scripts/windows.ps1 -Action Setup` is an explicit dependency installation, not an automatic safe check.
- Do not attach a host disk or real PCI hardware to experimental image-writing or GPU initialization paths.

## Start with current source

```bash
git switch main
git pull --ff-only origin main
git status --short
cat AGENTS.md
cat docs/STATUS.md
cat docs/specs/README.md
```

For an agent: **do not modify `main`**; create a new `agent/<domain>-im2-<topic>` branch and draft PR, according to [ownership](NEXT_WAVE_I_M2.md).

## Formatting, host tests, bare-metal

```bash
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
python3 -m unittest discover -s tests -v

cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-threads
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-preempt
```

CI [integration-gate.yml](../.github/workflows/integration-gate.yml) запускає також ServiceManager/Supervisor/PID host tests, Python регресії, QEMU Ring3 і fault-negative smoke. CI проходив на [PR #26](https://github.com/kabasik007/ZillaOs/pull/26); нові коміти потребують **власних** перевірок.

## Linux/WSL QEMU

```bash
bash scripts/build.sh demo
python3 scripts/run_vm.py --display gtk
```

Для безголових QEMU smoke/negative tests запускай відповідні CI workflows або дотримуйся [BUILD.md](BUILD.md). Окремі Cargo features `native-ring3*` — **навмисні експериментальні ISO**, не звичайний Desktop runtime. Не стверджуй "QEMU pass" без серійного логу й конкретних expected markers.

## Windows/VMware

```powershell
cd M:\projects\ZillaOs
git switch main
git pull --ff-only origin main
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action Check
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

`Run` — лише наявний ISO, `BuildRun` — новий ISO. Перед повторним запуском VMware вимкни VM повністю (Power Off), не Suspend. Зберігай console/screenshots і exact commit. Без фізичного запуску познач `VMware: NOT RUN`.

## Change lifecycle — spec-first

Для нової функції, зміни ABI, процесного стану, безпекової політики, device claims або сценарію GUI потрібна [специфікація](specs/README.md) до реалізації. **Не створюй окремий spec для механічного форматування**. Реальні рішення оформлюй ADR, поведінку — spec, докази — verification, історію — PR/commit.

У PR вкажи: base/head SHA, власника, goal і non-goals, API/ABI changes, негативні тести, CI links, hardware NOT RUN, rollback і залежні PR. Shared changes тільки через Chief Architect.

## Чого CI не доводить

- Успішний `cargo build` не дорівнює завантаженню UEFI.
- Host-tested IPC/ServiceManager моделі не доводять production syscall/runtime daemon.
- QEMU Ring3 fixture не гарантує isolated Desktop processes або SMP.
- Read-only AMD/VMware PCI discovery не дорівнює hardware modesetting чи acceleration.
- Знімок екрана VMware — корисне ручне підтвердження Desktop, але не доказ усіх security boundaries.

Більше: [STATUS](STATUS.md), [EVIDENCE_MATRIX](verification/EVIDENCE_MATRIX.md), [VALIDATION](VALIDATION.md).
