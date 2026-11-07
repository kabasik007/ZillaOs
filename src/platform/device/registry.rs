//! Bounded, single-writer device registry with generation-checked handles.
//! No hardware access, allocation or synchronization primitives. Integration
//! must keep its owner behind a kernel lock when SMP is introduced.
use crate::platform::pci::{PciAddress, PciClass, PciDeviceInfo, PciInventory};
use crate::platform::resources::{ResourceError, ResourceRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceId {
    pub slot: usize,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceState {
    Discovered,
    Matched,
    Claimed,
    ProbedReadOnly,
    Active,
    Faulted,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverMatch {
    /// Exact model and optional exact class. Vendor-only GPU claiming is not allowed.
    Exact {
        vendor_id: u16,
        device_id: u16,
        class: Option<PciClass>,
    },
    /// Generic class drivers must be explicitly registered and reviewed.
    Class(PciClass),
}

impl DriverMatch {
    pub const fn matches(self, info: PciDeviceInfo) -> bool {
        match self {
            Self::Exact {
                vendor_id,
                device_id,
                class,
            } => {
                info.identity.vendor_id == vendor_id
                    && info.identity.device_id == device_id
                    && match class {
                        Some(c) => {
                            c.base == info.identity.class.base
                                && c.subclass == info.identity.class.subclass
                                && c.programming_interface
                                    == info.identity.class.programming_interface
                        }
                        None => true,
                    }
            }
            Self::Class(c) => {
                c.base == info.identity.class.base
                    && c.subclass == info.identity.class.subclass
                    && c.programming_interface == info.identity.class.programming_interface
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    Full,
    DuplicateAddress,
    InventoryTruncated,
    StaleHandle,
    WrongState,
    DriverMismatch,
    NoDriverMatch,
    ResourceInvalid(ResourceError),
    ResourceBusy,
    NoResourceSlots,
    GenerationExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceView {
    pub id: DeviceId,
    pub info: PciDeviceInfo,
    pub state: DeviceState,
    pub driver: Option<DriverId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceSnapshot<const CAP: usize> {
    devices: [Option<DeviceView>; CAP],
    pub count: usize,
}

impl<const CAP: usize> DeviceSnapshot<CAP> {
    pub fn iter(&self) -> impl Iterator<Item = DeviceView> + '_ {
        self.devices.iter().flatten().copied()
    }
}

#[derive(Clone, Copy)]
struct DeviceSlot {
    generation: u32,
    info: Option<PciDeviceInfo>,
    state: DeviceState,
    driver: Option<DriverId>,
    retired: bool,
}

#[derive(Clone, Copy)]
struct Claim {
    device: DeviceId,
    range: ResourceRange,
}

pub struct DeviceRegistry<const DEVICES: usize, const CLAIMS: usize> {
    slots: [Option<DeviceSlot>; DEVICES],
    claims: [Option<Claim>; CLAIMS],
}

impl<const DEVICES: usize, const CLAIMS: usize> Default for DeviceRegistry<DEVICES, CLAIMS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const DEVICES: usize, const CLAIMS: usize> DeviceRegistry<DEVICES, CLAIMS> {
    pub const fn new() -> Self {
        Self {
            slots: [None; DEVICES],
            claims: [None; CLAIMS],
        }
    }

    fn slot(&self, id: DeviceId) -> Result<DeviceSlot, RegistryError> {
        match self.slots.get(id.slot).copied().flatten() {
            Some(slot) if slot.generation == id.generation && slot.info.is_some() => Ok(slot),
            _ => Err(RegistryError::StaleHandle),
        }
    }

    fn slot_mut(&mut self, id: DeviceId) -> Result<&mut DeviceSlot, RegistryError> {
        let slot = self
            .slots
            .get_mut(id.slot)
            .ok_or(RegistryError::StaleHandle)?;
        match slot {
            Some(slot) if slot.generation == id.generation && slot.info.is_some() => Ok(slot),
            _ => Err(RegistryError::StaleHandle),
        }
    }

    pub fn get(&self, id: DeviceId) -> Result<DeviceView, RegistryError> {
        let slot = self.slot(id)?;
        Ok(DeviceView {
            id,
            info: slot.info.ok_or(RegistryError::StaleHandle)?,
            state: slot.state,
            driver: slot.driver,
        })
    }

    pub fn find(&self, address: PciAddress) -> Option<DeviceId> {
        self.slots.iter().enumerate().find_map(|(index, slot)| {
            let slot = (*slot)?;
            if slot.info?.identity.address == address {
                Some(DeviceId {
                    slot: index,
                    generation: slot.generation,
                })
            } else {
                None
            }
        })
    }

    pub fn insert(&mut self, info: PciDeviceInfo) -> Result<DeviceId, RegistryError> {
        if self.find(info.identity.address).is_some() {
            return Err(RegistryError::DuplicateAddress);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| match slot {
                None => true,
                Some(s) => s.info.is_none() && !s.retired,
            })
            .ok_or(RegistryError::Full)?;
        let generation = self.slots[index].map_or(1, |slot| slot.generation);
        self.slots[index] = Some(DeviceSlot {
            generation,
            info: Some(info),
            state: DeviceState::Discovered,
            driver: None,
            retired: false,
        });
        Ok(DeviceId {
            slot: index,
            generation,
        })
    }

    /// Import a complete scan transactionally: reject truncation, collision
    /// and capacity shortage before writing any registry slot.
    pub fn import_inventory<const CAP: usize>(
        &mut self,
        inventory: &PciInventory<CAP>,
    ) -> Result<(), RegistryError> {
        if inventory.truncated {
            return Err(RegistryError::InventoryTruncated);
        }
        let vacant = self
            .slots
            .iter()
            .filter(|slot| match slot {
                None => true,
                Some(s) => s.info.is_none() && !s.retired,
            })
            .count();
        if vacant < inventory.count {
            return Err(RegistryError::Full);
        }
        for (i, info) in inventory.iter().enumerate() {
            if self.find(info.identity.address).is_some() {
                return Err(RegistryError::DuplicateAddress);
            }
            for earlier in inventory.iter().take(i) {
                if earlier.identity.address == info.identity.address {
                    return Err(RegistryError::DuplicateAddress);
                }
            }
        }
        for info in inventory.iter() {
            self.insert(info)?;
        }
        Ok(())
    }

    pub fn match_driver(
        &mut self,
        id: DeviceId,
        driver: DriverId,
        rule: DriverMatch,
    ) -> Result<(), RegistryError> {
        let slot = self.slot_mut(id)?;
        if slot.state != DeviceState::Discovered {
            return Err(RegistryError::WrongState);
        }
        if !rule.matches(slot.info.ok_or(RegistryError::StaleHandle)?) {
            return Err(RegistryError::NoDriverMatch);
        }
        slot.state = DeviceState::Matched;
        slot.driver = Some(driver);
        Ok(())
    }

    /// Atomic metadata-only exclusive claims. All requests are validated first.
    /// No BAR sizing/mapping or IRQ/DMA programming happens.
    pub fn claim(
        &mut self,
        id: DeviceId,
        driver: DriverId,
        requested: &[ResourceRange],
    ) -> Result<(), RegistryError> {
        let slot = self.slot(id)?;
        if slot.state != DeviceState::Matched {
            return Err(RegistryError::WrongState);
        }
        if slot.driver != Some(driver) {
            return Err(RegistryError::DriverMismatch);
        }
        if requested.len() > self.claims.iter().filter(|c| c.is_none()).count() {
            return Err(RegistryError::NoResourceSlots);
        }
        for (i, range) in requested.iter().copied().enumerate() {
            range
                .valid_for_claim()
                .map_err(RegistryError::ResourceInvalid)?;
            for earlier in requested.iter().take(i) {
                if range.overlaps(*earlier) {
                    return Err(RegistryError::ResourceBusy);
                }
            }
            for active in self.claims.iter().flatten() {
                if range.overlaps(active.range) {
                    return Err(RegistryError::ResourceBusy);
                }
            }
        }

        let mut next = 0usize;
        for range in requested.iter().copied() {
            while self.claims[next].is_some() {
                next += 1;
            }
            self.claims[next] = Some(Claim { device: id, range });
            next += 1;
        }
        self.slot_mut(id)?.state = DeviceState::Claimed;
        Ok(())
    }

    pub fn mark_probed_read_only(
        &mut self,
        id: DeviceId,
        driver: DriverId,
    ) -> Result<(), RegistryError> {
        let slot = self.slot_mut(id)?;
        if slot.driver != Some(driver) {
            return Err(RegistryError::DriverMismatch);
        }
        if slot.state != DeviceState::Claimed {
            return Err(RegistryError::WrongState);
        }
        slot.state = DeviceState::ProbedReadOnly;
        Ok(())
    }

    fn release_claims(&mut self, id: DeviceId) {
        for claim in &mut self.claims {
            if claim.is_some_and(|entry| entry.device == id) {
                *claim = None;
            }
        }
    }

    /// Revokes all claims, increments generation, and marks faulted.
    /// No live MMIO/DMA can exist in this model; real driver teardown is future work.
    pub fn mark_faulted(&mut self, id: DeviceId) -> Result<DeviceId, RegistryError> {
        let slot = self.slot(id)?;
        if slot.generation == u32::MAX {
            return Err(RegistryError::GenerationExhausted);
        }
        self.release_claims(id);
        let slot = self.slot_mut(id)?;
        slot.generation += 1;
        slot.state = DeviceState::Faulted;
        slot.driver = None;
        Ok(DeviceId {
            slot: id.slot,
            generation: slot.generation,
        })
    }

    pub fn reset_fault(&mut self, id: DeviceId) -> Result<(), RegistryError> {
        let slot = self.slot_mut(id)?;
        if slot.state != DeviceState::Faulted {
            return Err(RegistryError::WrongState);
        }
        slot.state = DeviceState::Discovered;
        Ok(())
    }

    /// Invalidate the handle and vacate slot. Saturated generations retire
    /// the slot forever rather than allowing u32 wrap/stale-handle aliasing.
    pub fn remove(&mut self, id: DeviceId) -> Result<(), RegistryError> {
        self.slot(id)?;
        self.release_claims(id);
        let slot = self.slot_mut(id)?;
        slot.info = None;
        slot.state = DeviceState::Removed;
        slot.driver = None;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
        } else {
            slot.retired = true;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> DeviceSnapshot<DEVICES> {
        let mut devices = [None; DEVICES];
        let mut count = 0;
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(slot) = slot {
                if let Some(info) = slot.info {
                    devices[index] = Some(DeviceView {
                        id: DeviceId {
                            slot: index,
                            generation: slot.generation,
                        },
                        info,
                        state: slot.state,
                        driver: slot.driver,
                    });
                    count += 1;
                }
            }
        }
        DeviceSnapshot { devices, count }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::pci::{PciAddress, PciIdentity};
    use crate::platform::resources::{ResourceKind, ResourceSource};

    fn sample(bus: u8, device: u8, vendor: u16, model: u16) -> PciDeviceInfo {
        PciDeviceInfo {
            identity: PciIdentity {
                address: PciAddress::new(0, bus, device, 0).unwrap(),
                vendor_id: vendor,
                device_id: model,
                revision: 0,
                class: PciClass {
                    base: 3,
                    subclass: 0,
                    programming_interface: 0,
                },
                subsystem_vendor_id: None,
                subsystem_device_id: None,
            },
            header_type: 0,
            multifunction: false,
            bridge: None,
        }
    }
    fn match_rule(vendor: u16, device: u16) -> DriverMatch {
        DriverMatch::Exact {
            vendor_id: vendor,
            device_id: device,
            class: None,
        }
    }
    fn mmio(base: u64) -> ResourceRange {
        ResourceRange::new(
            ResourceKind::Mmio,
            base,
            4096,
            ResourceSource::FirmwareValidated,
        )
        .unwrap()
    }

    #[test]
    fn duplicate_and_stale_handles_are_denied() {
        let mut r = DeviceRegistry::<2, 2>::new();
        let a = sample(0, 2, 0x15ad, 0x0405);
        let id = r.insert(a).unwrap();
        assert_eq!(r.insert(a), Err(RegistryError::DuplicateAddress));
        r.remove(id).unwrap();
        assert_eq!(r.get(id), Err(RegistryError::StaleHandle));
        let replacement = r.insert(a).unwrap();
        assert_eq!(replacement.slot, id.slot);
        assert_ne!(replacement.generation, id.generation);
        assert_eq!(r.get(id), Err(RegistryError::StaleHandle));
    }

    #[test]
    fn strict_match_and_atomic_exclusive_claim() {
        let mut r = DeviceRegistry::<4, 4>::new();
        let amd = r.insert(sample(0, 2, 0x1002, 0x73bf)).unwrap();
        let vmware = r.insert(sample(0, 3, 0x15ad, 0x0405)).unwrap();
        let owner = DriverId(7);
        assert_eq!(
            r.match_driver(amd, owner, match_rule(0x1002, 0x1234)),
            Err(RegistryError::NoDriverMatch)
        );
        r.match_driver(amd, owner, match_rule(0x1002, 0x73bf))
            .unwrap();
        assert_eq!(
            r.claim(amd, DriverId(8), &[mmio(0x1000)]),
            Err(RegistryError::DriverMismatch)
        );
        assert_eq!(
            r.claim(amd, owner, &[mmio(0x1000), mmio(0x1800)]),
            Err(RegistryError::ResourceBusy)
        );
        assert_eq!(r.get(amd).unwrap().state, DeviceState::Matched);
        r.claim(amd, owner, &[mmio(0x1000)]).unwrap();
        r.mark_probed_read_only(amd, owner).unwrap();

        r.match_driver(vmware, DriverId(8), match_rule(0x15ad, 0x0405))
            .unwrap();
        assert_eq!(
            r.claim(vmware, DriverId(8), &[mmio(0x1000)]),
            Err(RegistryError::ResourceBusy)
        );
        r.claim(vmware, DriverId(8), &[mmio(0x2000)]).unwrap();
        let old_snapshot = r.snapshot();
        let new_id = r.mark_faulted(amd).unwrap();
        assert_eq!(r.get(amd), Err(RegistryError::StaleHandle));
        assert_eq!(r.get(new_id).unwrap().state, DeviceState::Faulted);
        assert_eq!(old_snapshot.count, 2);
        assert_eq!(
            old_snapshot.iter().next().unwrap().state,
            DeviceState::ProbedReadOnly
        );
        r.reset_fault(new_id).unwrap();
    }

    #[test]
    fn malformed_unverified_resources_and_capacity_fail_closed() {
        let mut r = DeviceRegistry::<1, 0>::new();
        let id = r.insert(sample(0, 0, 0x1002, 0xbeef)).unwrap();
        r.match_driver(id, DriverId(1), match_rule(0x1002, 0xbeef))
            .unwrap();
        assert_eq!(
            r.claim(id, DriverId(1), &[mmio(0x1000)]),
            Err(RegistryError::NoResourceSlots)
        );
        let invalid =
            ResourceRange::new(ResourceKind::Mmio, 0x1000, 4096, ResourceSource::Unverified)
                .unwrap();
        assert_eq!(
            r.claim(id, DriverId(1), &[invalid]),
            Err(RegistryError::NoResourceSlots)
        );
        r.claim(id, DriverId(1), &[]).unwrap(); // metadata-only read-only claim
        assert_eq!(r.get(id).unwrap().state, DeviceState::Claimed);
    }

    #[test]
    fn registry_snapshot_is_copy_not_mutable_registry_reference() {
        let mut r = DeviceRegistry::<1, 1>::new();
        let id = r.insert(sample(0, 4, 0x15ad, 0x0405)).unwrap();
        let snapshot = r.snapshot();
        r.remove(id).unwrap();
        assert_eq!(snapshot.count, 1);
        assert_eq!(snapshot.iter().next().unwrap().id, id);
        assert_eq!(r.snapshot().count, 0);
    }
}
