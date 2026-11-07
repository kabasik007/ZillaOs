//! Bounded kernel-owned capability model. No user pointers, allocator or syscall hooks.
//! All operations are trusted kernel APIs; never expose grant/revoke as a user syscall.
pub mod process_binding;
pub mod typed;

pub const MAX_OBJECTS: usize = 128;
pub const MAX_HANDLES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Endpoint, Process, Thread, Device, Service, Irq, Mmio, Dma, File, Surface, Timer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rights(u64);
impl Rights {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const SEND: Self = Self(1 << 2);
    pub const RECEIVE: Self = Self(1 << 3);
    pub const TRANSFER: Self = Self(1 << 4);
    pub const CONTROL: Self = Self(1 << 5);
    pub const OBSERVE: Self = Self(1 << 6);
    pub const KNOWN: u64 = (1 << 7) - 1;
    pub const fn bits(self) -> u64 { self.0 }
    pub const fn from_bits(bits: u64) -> Option<Self> {
        if bits & !Self::KNOWN == 0 { Some(Self(bits)) } else { None }
    }
    pub const fn union(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
    pub const fn intersect(self, rhs: Self) -> Self { Self(self.0 & rhs.0) }
    pub const fn contains(self, rhs: Self) -> bool { self.0 & rhs.0 == rhs.0 }
    pub const fn is_empty(self) -> bool { self.0 == 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    InvalidOwner, InvalidHandle, StaleHandle, InvalidObject, Revoked,
    WrongType, AccessDenied, EmptyRights, Full, Exhausted,
}

/// Only meaningful inside its owning process's table; NOT a global authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u64);
impl Handle {
    pub const fn from_raw(raw: u64) -> Self { Self(raw) }
    pub const fn raw(self) -> u64 { self.0 }
    const fn new(slot: usize, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (slot as u64 + 1))
    }
    fn parts(self) -> Result<(usize, u32), CapError> {
        let low = self.0 as u32;
        let generation = (self.0 >> 32) as u32;
        if low == 0 || generation == 0 { return Err(CapError::InvalidHandle); }
        let slot = (low - 1) as usize;
        if slot >= MAX_HANDLES { return Err(CapError::InvalidHandle); }
        Ok((slot, generation))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectKey {
    slot: u32,
    generation: u32,
    id: ObjectId,
    kind: ObjectKind,
}
impl ObjectKey {
    pub const fn id(self) -> ObjectId { self.id }
    pub const fn kind(self) -> ObjectKind { self.kind }
}


#[derive(Clone, Copy)]
struct ObjectSlot {
    generation: u32,
    object: Option<(ObjectId, ObjectKind)>,
    owner: Option<u64>,
    retired: bool,
}
const EMPTY_OBJECT: ObjectSlot = ObjectSlot { generation: 1, object: None, owner: None, retired: false };

/// Shared kernel object registry. Revoke invalidates ALL delegated references.
pub struct ObjectRegistry {
    slots: [ObjectSlot; MAX_OBJECTS],
    next_id: u64,
}
impl Default for ObjectRegistry { fn default() -> Self { Self::new() } }
impl ObjectRegistry {
    pub const fn new() -> Self {
        Self { slots: [EMPTY_OBJECT; MAX_OBJECTS], next_id: 1 }
    }
    pub fn create(&mut self, kind: ObjectKind) -> Result<ObjectKey, CapError> {
        self.create_with_owner(kind, None)
    }
    /// Trusted issuer only. Ownership is resource metadata; no process records are created.
    pub fn create_owned(&mut self, kind: ObjectKind, owner: u64)
        -> Result<ObjectKey, CapError>
    {
        if owner == 0 { return Err(CapError::InvalidOwner); }
        self.create_with_owner(kind, Some(owner))
    }
    fn create_with_owner(&mut self, kind: ObjectKind, owner: Option<u64>)
        -> Result<ObjectKey, CapError>
    {
        let index = self.slots.iter().position(|s| s.object.is_none() && !s.retired)
            .ok_or(CapError::Full)?;
        let next = self.next_id.checked_add(1).ok_or(CapError::Exhausted)?;
        let id = ObjectId(self.next_id);
        self.next_id = next;
        let slot = &mut self.slots[index];
        slot.object = Some((id, kind));
        slot.owner = owner;
        Ok(ObjectKey { slot: index as u32, generation: slot.generation, id, kind })
    }
    pub fn validate(&self, key: ObjectKey) -> Result<(), CapError> {
        let slot = self.slots.get(key.slot as usize).ok_or(CapError::InvalidObject)?;
        if slot.generation != key.generation { return Err(CapError::Revoked); }
        if slot.object != Some((key.id, key.kind)) { return Err(CapError::Revoked); }
        Ok(())
    }
    /// Read ownership by a kernel-minted key; never authorize by ObjectId alone.
    pub fn owner_of(&self, key: ObjectKey) -> Result<Option<u64>, CapError> {
        self.validate(key)?;
        Ok(self.slots[key.slot as usize].owner)
    }
    /// Kernel-owned queue discovery only: not an authorization query.
    pub fn owner_of_id(&self, id: ObjectId) -> Option<u64> {
        self.slots.iter().find_map(|slot| match slot.object {
            Some((object_id, _)) if object_id == id => slot.owner,
            _ => None,
        })
    }
    /// Object revocation on verified process exit; no DMA/IRQ teardown is implied.
    pub fn revoke_owned(&mut self, owner: u64) -> usize {
        if owner == 0 { return 0; }
        let mut count = 0;
        for slot in &mut self.slots {
            if slot.owner == Some(owner) && slot.object.is_some() {
                slot.object = None;
                slot.owner = None;
                if slot.generation == u32::MAX { slot.retired = true; }
                else { slot.generation += 1; }
                count += 1;
            }
        }
        count
    }
    pub fn revoke(&mut self, key: ObjectKey) -> Result<(), CapError> {
        self.validate(key)?;
        let slot = &mut self.slots[key.slot as usize];
        slot.object = None;
        slot.owner = None;
        if slot.generation == u32::MAX { slot.retired = true; }
        else { slot.generation += 1; }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delegation {
    key: ObjectKey,
    rights: Rights,
}
impl Delegation {
    pub fn validate(self, objects: &ObjectRegistry) -> Result<(), CapError> {
        objects.validate(self.key)
    }
    pub const fn rights(self) -> Rights { self.rights }
    pub const fn object_id(self) -> ObjectId { self.key.id }
}

#[derive(Clone, Copy)]
struct HandleSlot {
    generation: u32,
    value: Option<Delegation>,
    retired: bool,
}
const EMPTY_HANDLE: HandleSlot = HandleSlot { generation: 1, value: None, retired: false };

/// Per-process handle table. Caller PID is supplied by a trusted kernel task source.
pub struct HandleTable {
    owner: u64,
    slots: [HandleSlot; MAX_HANDLES],
}
impl HandleTable {
    pub const fn new(owner: u64) -> Self {
        Self { owner, slots: [EMPTY_HANDLE; MAX_HANDLES] }
    }
    pub const fn owner(&self) -> u64 { self.owner }
    fn check_owner(&self, caller: u64) -> Result<(), CapError> {
        if caller == 0 || caller != self.owner { Err(CapError::InvalidOwner) } else { Ok(()) }
    }
    fn insert(&mut self, grant: Delegation) -> Result<Handle, CapError> {
        let index = self.slots.iter().position(|s| s.value.is_none() && !s.retired)
            .ok_or(CapError::Full)?;
        let slot = &mut self.slots[index];
        slot.value = Some(grant);
        Ok(Handle::new(index, slot.generation))
    }
    /// Trusted issuer only: caller cannot request this operation from user space.
    pub fn grant(&mut self, caller: u64, objects: &ObjectRegistry, key: ObjectKey, rights: Rights)
        -> Result<Handle, CapError>
    {
        self.check_owner(caller)?;
        objects.validate(key)?;
        if rights.is_empty() { return Err(CapError::EmptyRights); }
        self.insert(Delegation { key, rights })
    }
    fn lookup(&self, caller: u64, objects: &ObjectRegistry, handle: Handle)
        -> Result<Delegation, CapError>
    {
        self.check_owner(caller)?;
        let (index, generation) = handle.parts()?;
        let slot = &self.slots[index];
        if slot.generation != generation { return Err(CapError::StaleHandle); }
        let grant = slot.value.ok_or(CapError::StaleHandle)?;
        objects.validate(grant.key)?;
        Ok(grant)
    }
    /// Enforced on EVERY use, including after a request was enqueued.
    pub fn authorize(&self, caller: u64, objects: &ObjectRegistry, handle: Handle,
        kind: ObjectKind, required: Rights) -> Result<ObjectId, CapError>
    {
        Ok(self.authorize_key(caller, objects, handle, kind, required)?.id())
    }
    /// Returns a generation-aware kernel key, not just an object number.
    /// Recheck under the resource lock before touching backing memory/hardware.
    pub fn authorize_key(&self, caller: u64, objects: &ObjectRegistry, handle: Handle,
        kind: ObjectKind, required: Rights) -> Result<ObjectKey, CapError>
    {
        let grant = self.lookup(caller, objects, handle)?;
        if grant.key.kind != kind { return Err(CapError::WrongType); }
        if !grant.rights.contains(required) { return Err(CapError::AccessDenied); }
        Ok(grant.key)
    }
    /// Source needs TRANSFER. Requested rights are intersected; never amplified.
    pub fn prepare_transfer(&self, caller: u64, objects: &ObjectRegistry,
        handle: Handle, requested: Rights) -> Result<Delegation, CapError>
    {
        let source = self.lookup(caller, objects, handle)?;
        if !source.rights.contains(Rights::TRANSFER) { return Err(CapError::AccessDenied); }
        let rights = source.rights.intersect(requested);
        if rights.is_empty() { return Err(CapError::EmptyRights); }
        Ok(Delegation { key: source.key, rights })
    }
    /// Trusted receiver commit after validating all references, not a user grant.
    pub fn accept(&mut self, caller: u64, objects: &ObjectRegistry, grant: Delegation)
        -> Result<Handle, CapError>
    {
        self.check_owner(caller)?;
        grant.validate(objects)?;
        if grant.rights.is_empty() { return Err(CapError::EmptyRights); }
        self.insert(grant)
    }
    /// Preflight ALL grants and receiver capacity before any handle is installed.
    /// Caller must serialize registry/table mutations; this is not an SMP lock.
    pub fn accept_many<const N: usize>(
        &mut self, caller: u64, objects: &ObjectRegistry,
        grants: &[Option<Delegation>; N],
    ) -> Result<[Option<Handle>; N], CapError> {
        self.check_owner(caller)?;
        let mut needed = 0usize;
        for grant in grants.iter().flatten() {
            grant.validate(objects)?;
            if grant.rights.is_empty() { return Err(CapError::EmptyRights); }
            needed += 1;
        }
        let available = self.slots.iter()
            .filter(|slot| slot.value.is_none() && !slot.retired).count();
        if needed > available { return Err(CapError::Full); }
        let mut result = [None; N];
        for (i, grant) in grants.iter().enumerate() {
            if let Some(grant) = grant {
                // The preflight guarantees free slots under the exclusive &mut borrow.
                result[i] = Some(self.insert(*grant)?);
            }
        }
        Ok(result)
    }
    pub fn close(&mut self, caller: u64, handle: Handle) -> Result<(), CapError> {
        self.check_owner(caller)?;
        let (index, generation) = handle.parts()?;
        let slot = &mut self.slots[index];
        if generation != slot.generation || slot.value.is_none() { return Err(CapError::StaleHandle); }
        slot.value = None;
        if slot.generation == u32::MAX { slot.retired = true; }
        else { slot.generation += 1; }
        Ok(())
    }
    /// Kernel process-exit cleanup; globally-owned objects must separately be revoked.
    pub fn close_all(&mut self) {
        for slot in &mut self.slots {
            if slot.value.take().is_some() {
                if slot.generation == u32::MAX { slot.retired = true; }
                else { slot.generation += 1; }
            }
        }
    }
    /// Reclaim slots whose global objects were revoked, preserving stale rejection.
    /// This is optional deterministic cleanup, not an automatic background worker.
    pub fn sweep_revoked(&mut self, objects: &ObjectRegistry) -> usize {
        let mut reclaimed = 0;
        for slot in &mut self.slots {
            if let Some(grant) = slot.value {
                if objects.validate(grant.key).is_err() {
                    slot.value = None;
                    if slot.generation == u32::MAX { slot.retired = true; }
                    else { slot.generation += 1; }
                    reclaimed += 1;
                }
            }
        }
        reclaimed
    }
    pub fn count(&self) -> usize { self.slots.iter().filter(|s| s.value.is_some()).count() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeniedEvent {
    pub caller: u64,
    pub object: Option<ObjectId>,
    pub reason: CapError,
}
/// Data only; an authenticated, bounded kernel EventLog must persist these later.
pub const fn denied_event(caller: u64, object: Option<ObjectId>, reason: CapError) -> DeniedEvent {
    DeniedEvent { caller, object, reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forgery_type_and_owner_are_denied() {
        let mut objects = ObjectRegistry::new();
        let endpoint = objects.create(ObjectKind::Endpoint).unwrap();
        let mut a = HandleTable::new(11);
        let h = a.grant(11, &objects, endpoint, Rights::SEND).unwrap();
        assert_eq!(a.authorize(12, &objects, h, ObjectKind::Endpoint, Rights::SEND), Err(CapError::InvalidOwner));
        assert_eq!(a.authorize(11, &objects, Handle::from_raw(0), ObjectKind::Endpoint, Rights::SEND), Err(CapError::InvalidHandle));
        assert_eq!(a.authorize(11, &objects, Handle::from_raw(h.raw() + 1), ObjectKind::Endpoint, Rights::SEND), Err(CapError::StaleHandle));
        assert_eq!(a.authorize(11, &objects, h, ObjectKind::Device, Rights::SEND), Err(CapError::WrongType));
        assert_eq!(a.authorize(11, &objects, h, ObjectKind::Endpoint, Rights::RECEIVE), Err(CapError::AccessDenied));
    }
    #[test]
    fn generation_close_and_revoke_reject_stale_handles() {
        let mut objects = ObjectRegistry::new();
        let akey = objects.create(ObjectKind::Device).unwrap();
        let mut a = HandleTable::new(1);
        let h = a.grant(1, &objects, akey, Rights::OBSERVE).unwrap();
        a.close(1, h).unwrap();
        assert_eq!(a.authorize(1, &objects, h, ObjectKind::Device, Rights::OBSERVE), Err(CapError::StaleHandle));
        let next = a.grant(1, &objects, akey, Rights::OBSERVE).unwrap();
        assert_ne!(next.raw(), h.raw());
        objects.revoke(akey).unwrap();
        assert_eq!(a.authorize(1, &objects, next, ObjectKind::Device, Rights::OBSERVE), Err(CapError::Revoked));
        let again = objects.create(ObjectKind::Device).unwrap();
        assert_ne!(akey.id(), again.id());
        assert_eq!(objects.validate(akey), Err(CapError::Revoked));
    }
    #[test]
    fn delegation_attenuates_and_global_revoke_propagates() {
        let mut objects = ObjectRegistry::new();
        let key = objects.create(ObjectKind::Endpoint).unwrap();
        let mut sender = HandleTable::new(1);
        let mut receiver = HandleTable::new(2);
        let original = sender.grant(1, &objects, key, Rights::TRANSFER.union(Rights::SEND)).unwrap();
        let grant = sender.prepare_transfer(1, &objects, original, Rights::SEND.union(Rights::CONTROL)).unwrap();
        assert_eq!(grant.rights(), Rights::SEND);
        let copied = receiver.accept(2, &objects, grant).unwrap();
        assert_eq!(receiver.authorize(2, &objects, copied, ObjectKind::Endpoint, Rights::SEND), Ok(key.id()));
        assert_eq!(receiver.authorize(2, &objects, copied, ObjectKind::Endpoint, Rights::CONTROL), Err(CapError::AccessDenied));
        objects.revoke(key).unwrap();
        assert_eq!(receiver.authorize(2, &objects, copied, ObjectKind::Endpoint, Rights::SEND), Err(CapError::Revoked));
    }
    #[test]
    fn bounded_and_cleanup() {
        let mut registry = ObjectRegistry::new();
        let key = registry.create(ObjectKind::File).unwrap();
        let mut t = HandleTable::new(1);
        for _ in 0..MAX_HANDLES { t.grant(1, &registry, key, Rights::READ).unwrap(); }
        assert_eq!(t.grant(1, &registry, key, Rights::READ), Err(CapError::Full));
        t.close_all();
        assert_eq!(t.count(), 0);
        for _ in 1..MAX_OBJECTS { registry.create(ObjectKind::Timer).unwrap(); }
        assert_eq!(registry.create(ObjectKind::Timer), Err(CapError::Full));
    }
    #[test]
    fn batch_receive_is_fail_closed_and_preflight_atomic() {
        let mut reg = ObjectRegistry::new();
        let akey = reg.create(ObjectKind::Endpoint).unwrap();
        let bkey = reg.create(ObjectKind::Endpoint).unwrap();
        let mut source = HandleTable::new(1);
        let mut recipient = HandleTable::new(2);
        let all = Rights::SEND.union(Rights::TRANSFER);
        let ha = source.grant(1, &reg, akey, all).unwrap();
        let hb = source.grant(1, &reg, bkey, all).unwrap();
        let grants = [
            Some(source.prepare_transfer(1, &reg, ha, Rights::SEND).unwrap()),
            Some(source.prepare_transfer(1, &reg, hb, Rights::SEND).unwrap()),
        ];
        reg.revoke(bkey).unwrap();
        assert!(matches!(recipient.accept_many(2, &reg, &grants), Err(CapError::Revoked)));
        assert_eq!(recipient.count(), 0);
        let revised = [grants[0], None];
        let accepted = recipient.accept_many(2, &reg, &revised).unwrap();
        assert!(accepted[0].is_some());
        assert!(accepted[1].is_none());
        assert_eq!(recipient.count(), 1);
    }
    #[test]
    fn revoked_sweep_reclaims_without_resurrecting_handles() {
        let mut reg = ObjectRegistry::new();
        let key = reg.create(ObjectKind::Device).unwrap();
        let mut t = HandleTable::new(1);
        let handle = t.grant(1, &reg, key, Rights::OBSERVE).unwrap();
        reg.revoke(key).unwrap();
        assert_eq!(t.sweep_revoked(&reg), 1);
        assert_eq!(t.count(), 0);
        assert_eq!(t.authorize(1, &reg, handle, ObjectKind::Device, Rights::OBSERVE),
            Err(CapError::StaleHandle));
        assert_eq!(t.sweep_revoked(&reg), 0);
    }
    #[test]
    fn generation_rollover_retires_slots_instead_of_recycling() {
        let mut reg = ObjectRegistry::new();
        let mut key = reg.create(ObjectKind::Endpoint).unwrap();
        let mut t = HandleTable::new(1);
        let first = t.grant(1, &reg, key, Rights::SEND).unwrap();
        t.slots[0].generation = u32::MAX;
        assert_eq!(t.close(1, first), Err(CapError::StaleHandle));
        let max_handle = Handle::new(0, u32::MAX);
        t.close(1, max_handle).unwrap();
        assert!(t.slots[0].retired);
        let next = t.grant(1, &reg, key, Rights::SEND).unwrap();
        assert_ne!(next.raw() as u32, 1);
        // Exhaust the registry object's generation safely as well.
        reg.slots[key.slot as usize].generation = u32::MAX;
        key.generation = u32::MAX;
        reg.revoke(key).unwrap();
        assert!(reg.slots[key.slot as usize].retired);
        let newer = reg.create(ObjectKind::Endpoint).unwrap();
        assert_ne!(newer.slot, key.slot);
    }
    #[test]
    fn owned_revocation_invalidates_all_copies_not_other_owners() {
        let mut reg = ObjectRegistry::new();
        let key = reg.create_owned(ObjectKind::Device, 10).unwrap();
        let other = reg.create_owned(ObjectKind::Surface, 11).unwrap();
        let unowned = reg.create(ObjectKind::Timer).unwrap();
        assert_eq!(reg.owner_of(key), Ok(Some(10)));
        let mut recipient = HandleTable::new(20);
        let leaked = recipient.grant(20, &reg, key, Rights::OBSERVE).unwrap();
        assert_eq!(reg.revoke_owned(10), 1);
        assert_eq!(reg.revoke_owned(10), 0);
        assert_eq!(recipient.authorize(20, &reg, leaked, ObjectKind::Device, Rights::OBSERVE),
            Err(CapError::Revoked));
        assert_eq!(reg.owner_of(other), Ok(Some(11)));
        assert_eq!(reg.owner_of(unowned), Ok(None));
    }
    #[test]
    fn close_does_not_revoke_valid_delegated_copy() {
        let mut reg = ObjectRegistry::new();
        let key = reg.create(ObjectKind::Endpoint).unwrap();
        let mut a = HandleTable::new(1);
        let mut b = HandleTable::new(2);
        let h = a.grant(1, &reg, key, Rights::SEND.union(Rights::TRANSFER)).unwrap();
        let copy = b.accept(2, &reg, a.prepare_transfer(1, &reg, h, Rights::SEND).unwrap()).unwrap();
        a.close(1, h).unwrap();
        assert_eq!(b.authorize(2, &reg, copy, ObjectKind::Endpoint, Rights::SEND), Ok(key.id()));
    }
}