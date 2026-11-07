# SPEC: єдиний read-only PCI inventory для boot

**ID:** I-M1-PCI-READONLY · **Status:** implemented-in-source, host/QEMU verified for segment-0 VM demo  
**Owner:** A — Platform/PCI; shared `amd_bridge` integration by Chief Architect  
**As of:** 2026-11-07  
**Evidence:** [Platform implementation](../../PLATFORM_IM1_SINGLE_INVENTORY.md), [CI](https://github.com/kabasik007/ZillaOs/actions/runs/42336227877), [source](../../../src/platform/boot.rs), [Issue #20](https://github.com/kabasik007/ZillaOs/issues/20) (closed for read-only scope).

## Goal

Під час `vm-demo` Platform виконує **одну bounded read-only інвентаризацію legacy PCI segment 0** і надає єдиний `DeviceRegistry`; Storage/VMware/AMD отримують immutable snapshots / adapted identities без другого апаратного сканування.

## Observable rules

1. Кожен розпізнаний PCI endpoint має один canonical segment/BDF/vendor/device/class identity; consumers не змінюють цю identity.
2. `BootPci` не може непомітно зрізати overflow/malformed topology: status позначає неповний чи некоректний scan, замість вигаданої повноти.
3. VMware consumer використовує кешовані PCI dword values, AMD діагностика отримує identity через Platform adapter; read-only snapshots не породжують конфігураційних записів.
4. `DeviceId` має generation-safe semantics; застарілий handle не повертає доступу до іншого пристрою.

## Non-goals

- Не вводимо PCI config device writes, BAR sizing, DMA, MMIO, IRQ, bus mastering, firmware чи GPU initialization.
- Не обіцяємо багатосегментний PCIe/ACPI ECAM, hotplug, safe DMA isolation або фізичну AMD підтримку.

## Negative behavior / invariants

- Inventory overflow / invalid header / malformed bridge / read error → bounded error status.
- Stale `DeviceId` / invalid BDF / зміна identity → deny; no resource grant.
- One writer during single-core boot; майбутній SMP потребує синхронізованого owner gateway.

## Verification

- Platform QEMU/OVMF smoke and host negative tests — **PASS** на [PR #26 code baseline](https://github.com/kabasik007/ZillaOs/actions/runs/42336227877).
- Real physical AMD, VMware Workstation after this integration, active hardware DMA/IRQ — **NOT RUN**.
- Future changes require same host, bare-metal and QEMU parity tests without disabling software framebuffer fallback.
