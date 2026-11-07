# ZillaOS — PCIe, ACPI, ECAM, resource ownership та DMA/IOMMU

**Статус:** архітектурний проєкт; не апаратний драйвер. Дата ревізії: 2026-11-07. Для першої імплементації — тільки bounded read-only discovery.

## 1. Чому це передумова AMD GPU

У поточному ZillaOS `src/pci_io.rs` використовує ранній x86 PCI config I/O; `src/drivers/storage_pci.rs` надає `PciLocation`; `src/video/mod.rs` шукає клас дисплея; `src/drivers/gpu/vmware_svga/pci.rs` виявляє SVGA. Відсутній центральний kernel device/resource authority. Спроба AMD-драйвера без такого шару створить третій сканер та незахищені BAR mappings.

**Ціль:** один inventory → stable DeviceId/generation → resources → claims → драйвери. Ця модель з часом обслуговуватиме GPU, NVMe, USB, NIC, audio, а не лише AMD.

## 2. PCI config access: CF8/CFC vs ECAM

### Існуючий bootstrap: legacy mechanism #1

- x86 `0xCF8/0xCFC` — legacy PCI config I/O у segment/domain 0; лише перші 256 байт функції PCI; не вважати його універсальним конфігураційним інтерфейсом для всіх PCIe bridges/capabilities.
- `bus:u8/device:0..31/function:0..7`; `offset` має бути aligned для dword read. Для майбутніх 8/16-bit полів робити mask/shift із чітко визначеним endianness.
- Зберегти `src/pci_io.rs` як один early adapter, не додавати прямі `in/out` у `gpu/amd`.
- Для багатоядерного доступу до CF8/CFC потрібен **глобальний lock/serialization**, бо це спільна адресна пара; IRQ/preemption safety must be designed, не просто `&mut` у локальному Rust-методі.

### Ціль: ACPI MCFG + PCIe ECAM

Firmware надає MCFG allocations для PCIe MMIO config space. Кожна allocation описує base, segment group та bus range. Існують і platform host-bridge methods, тож MCFG **не замінює** повний ACPI host bridge/resource discovery.

- ACPI RSDP → XSDT/RSDT → SDT headers з перевіркою signature, length, checksum, overflow та незареєстрованих фізичних адрес.
- Validate `MCFG` checksum, header length, 16-byte entries, `start_bus <= end_bus`, reserved fields, non-overlap within a segment, full mapped extent. Не припускати, що всі 256 buses доступні.
- ECAM mapping size = `(bus_count * 1 MiB)` per segment entry. **Base з MCFG відповідає bus 0** (у загальному випадку), тому адреса пристрою використовує абсолютне bus number; не віднімати `start_bus` від формули без перевірки платформного контракту.
- Standard ECAM function size: 4 KiB. `base + (bus<<20) + (device<<15) + (function<<12) + offset`; усі арифметичні операції checked та bounds-checked.
- `segment != 0` має підтримуватись у DeviceId і PCI address struct від початку, навіть якщо bootstrap adapter поверне `UnsupportedSegment`.
- Prioritize valid firmware resource description, but validate against kernel memory map. До готовності `MMIOMapper` ECAM читати **не можна**.
- Адаптер конфігурації: `trait PciConfigRead { fn read32(&self, addr, offset) -> Result<u32, PciError>; }` — **ескіз**, не публічний ABI. ACPI parser без апаратних доступів тестувати окремо.

Референси: [ACPI 6.6](https://uefi.org/specifications), [Linux PCI/ACPI host bridge discussion](https://github.com/torvalds/linux/blob/master/Documentation/PCI/acpi-info.rst).

## 3. Enumeration як topology traversal

```text
PCI host bridge (segment + bus ranges)
   ├─ bus N → device 0..31 → function 0..7 (with multifunction check)
   ├─ PCI-to-PCI bridges: validated primary/secondary/subordinate buses
   ├─ endpoints: class/subclass/prog-if, IDs, revision
   └─ stable snapshot -> DeviceRegistry
```

- Перший етап допускає bounded read-only скан segment 0; наступний — коректне обмеження bus ranges за bridge topology.
- `0xffff` vendor -> absent; `header_type & 0x7f` визначає формат header; function 1..7 сканувати лише якщо multifunction bit set для function 0, якщо цього вимагає перевірений алгоритм.
- Bridges можуть мати malformed bus numbers або cycles: visited bus bitmap, max depth, device cap, explicit `Truncated`, no infinite recursion.
- Log deterministically: `pci=segment:bus:device.function, vendor, device, revision, class`.
- Hotplug (ACPI notification + PCIe ports) — окремий milestone, без blind polling і without freeing in-use driver memory.
- Base state: `Discovered`; `Bound` лише якщо драйвер підтримує *конкретний* ID/revision, а не просто vendor match.

## 4. Ownership & resource transactions

Кожен пристрій має stable `DeviceId`, generation і `DeviceState`. GPU/driver claim відрізняється від ресурсу. Model `ResourceSet` містить:
- BAR0..BAR5 descriptor: Memory32/Memory64/IO, prefetchable, `bus_address`, validated CPU physical resource extent, size, attributes, source of truth.
- ROM BAR, command/status register flags; ownership не дає права змінювати все.
- INTx/MSI/MSI-X capability descriptor, IRQ allocation token.
- DMA mask/address capabilities та IOMMU group/domain policy, якщо підтверджено.

**Попередження:** BAR readback дає адресний дескриптор, але не гарантовану довжину. PCI BAR sizing через тимчасові writes `0xffff...` не належить до read-only discovery й може пошкодити активне залізо, якщо виконати на чужому пристрої. Проводити ресурсну транзакцію лише під exclusive ownership/stop DMA, із відновленням початкового config та ретельними тестами; краще опиратися на валідований allocator/resource map.

```text
discover -> immutable snapshot -> candidate binding
       -> atomic claim resources -> validate -> optional map
       -> init gated by per-device policy -> active
       -> quiesce DMA/IRQ -> unmap -> release -> generation++
```

Device claim must fail on overlap; no driver gets raw physical addresses through generic IPC. Changes are serialized and audited.

## 5. MMIO mapping та CPU memory attributes

- Validate resource provenance, `base + length` overflow, page alignment and partial-page offsets.
- UC device registers, WC framebuffer *only with verified cache policy*; no accidental WB RAM caching for MMIO.
- Aliasing same physical range with incompatible cache attributes is forbidden.
- `volatile` alone does not provide CPU ordering, DMA coherence or thread safety.
- Resource mapping belongs to memory/DeviceManager kernel authority; GPU driver receives a typed `MappedBar` capability, not arbitrary `*mut u32`.
- Hot unplug/reset invalidates generation/handles; failing to invalidate could lead to use-after-free of MMIO.

## 6. IRQs, DMA та IOMMU

- Legacy INTx first as abstract model; MSI/MSI-X requires capabilities parser, IRQ vector owner, proper acknowledge/masking, interrupt-safe synchronization and bounded bottom-half queues.
- A GPU uses DMA and GPU virtual memory; before bus mastering enable, set up pinning/ownership, DMA mask, mapping/unmapping, coherence/fences and IOMMU policy.
- `CPU virtual`, `CPU physical`, `PCI bus` та `I/O virtual (IOVA)` addresses are not interchangeable.
- AMD-Vi (IOMMU) belongs to platform I/O isolation, not the same as AMDGPU Graphics VM or x86 page tables. APU/SOC-specific topology must be validated.
- ACPI IVRS (AMD IOMMU information) parsing, IOMMU page tables/domains and interrupt remapping — **future**. До цього default deny для неперевірених DMA devices.
- User-space driver isolation is not real without controlling DMA; навіть Ring 3 не захищає RAM від неізольованого PCI bus master.

Джерела: [Dynamic DMA mapping](https://docs.kernel.org/core-api/dma-api-howto.html), [PCI region claims](https://kernel.org/doc/html/latest/driver-api/pci/pci.html).

## 7. Запропоновані типи та module placement

```rust
// API SKETCH — compiled implementation NOT yet present.
#[derive(Clone, Copy, Eq, PartialEq)]
struct PciAddress { segment: u16, bus: u8, device: u8, function: u8 }

struct DeviceId { slot: u32, generation: u32 }
struct DeviceInfo { address: PciAddress, vendor: u16, device: u16, revision: u8 }
enum ResourceError { UnknownDevice, Busy, Overlap, InvalidRange, UnverifiedSize, Unsupported }
enum DeviceBinding { Unbound, Candidate, Claimed, ProbedReadOnly, Initialized, Faulted }
```

```text
src/bus/pci/{address.rs, config.rs, enumerate.rs, bridges.rs, mcfg.rs}  # proposal
src/kernel/device/{registry.rs, claim.rs, resources.rs}               # proposal
src/kernel/hal/{mmio.rs, irq.rs}                                      # proposal
src/kernel/dma/{mapping.rs, iommu.rs}                                 # proposal
```

`deviced` згодом бачить лише read-only snapshot і контролює policy/notification; право memory map, IRQ allocation і DMA domain залишається в kernel.

## 8. Failure-first validation matrix

| Input/condition | Expected fail-closed result |
| --- | --- |
| MCFG truncated/invalid checksum | reject table, keep validated CF8-only mode |
| Segment 1 on CF8 adapter | `UnsupportedSegment`, no config read |
| Unaligned ECAM access / offset >=4096 | no mapping, reject |
| malformed bridge bus range/cycle | bounded error/truncated snapshot |
| 64-bit BAR high dword absent | no mapping |
| BAR extent overlaps RAM/another claim | reject |
| resource length unknown | no map |
| two drivers claim same BAR | second denied |
| driver removed with stale DeviceId | generation mismatch, denied |
| bus master requested without DMA policy | denied |
| IOMMU table missing/invalid | no untrusted DMA isolation claim |
| AMD graphics vendor detected but ID unsupported | read-only diagnostic only |

Host unit tests on fake config/MCFG; bare-metal compile; QEMU tests on emulated PCI; **AMD physical evidence separately**. Do not use production disks or change host firmware.

## 9. Incremental tasks for the Platform/PCI agent

**P0:** pure `PciAddress`, `PciIdentity`, `PciConfigRead` plus adapter from existing `PciLocation`; no change to existing readers; tests.

**P1:** bounded PCI discovery, bridges and multifunction fixtures; report overflow; snapshot equality tests (stable order).

**P2:** kernel DeviceRegistry pure state + generation claims; tests resource conflicts/hot removal.

**P3:** ACPI RSDP/XSDT/MCFG parsing in isolation with fuzz/negative tests; keep ECAM hardware disabled.

**P4:** verified MMIO resource mapping / interrupt and DMA enforcement separately; only then device initialization experiments.

**Integration note:** `src/pci_io.rs`, `src/main.rs`, `src/lib.rs` and existing VMware/Storage consumer modules are owned by other agents; integration patches must be minimal and current-HEAD based.

## 10. Source map

- UEFI Forum: https://uefi.org/specifications
- PCI-SIG specs index: https://pcisig.com/specifications
- Linux PCI/ACPI host bridges: https://github.com/torvalds/linux/blob/master/Documentation/PCI/acpi-info.rst
- PCI driver resource API: https://kernel.org/doc/html/latest/driver-api/pci/pci.html
- DMA/IOMMU memory addresses: https://docs.kernel.org/core-api/dma-api-howto.html

**These are design references, not dependencies on Linux code.** Also read [DRIVER_FRAMEWORK.md](DRIVER_FRAMEWORK.md), [AMD_GPU_BRINGUP.md](AMD_GPU_BRINGUP.md), [OS_SUBSYSTEMS.md](OS_SUBSYSTEMS.md).
