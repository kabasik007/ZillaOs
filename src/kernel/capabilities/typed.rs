//! Typed resource views are NOT grants. Every operation rechecks the live
//! object's kind, generation and required rights against the caller's table.
use super::{CapError, Handle, HandleTable, ObjectKey, ObjectKind, ObjectRegistry, Rights};
use core::marker::PhantomData;

pub trait ResourceTag {
    const KIND: ObjectKind;
}
pub enum EndpointResource {}
pub enum DeviceResource {}
pub enum ServiceResource {}
pub enum SurfaceResource {}
pub enum ProcessResource {}
impl ResourceTag for EndpointResource {
    const KIND: ObjectKind = ObjectKind::Endpoint;
}
impl ResourceTag for DeviceResource {
    const KIND: ObjectKind = ObjectKind::Device;
}
impl ResourceTag for ServiceResource {
    const KIND: ObjectKind = ObjectKind::Service;
}
impl ResourceTag for SurfaceResource {
    const KIND: ObjectKind = ObjectKind::Surface;
}
impl ResourceTag for ProcessResource {
    const KIND: ObjectKind = ObjectKind::Process;
}

/// A typed view over an UNTRUSTED raw local handle value, not authority.
#[derive(Debug, PartialEq, Eq)]
pub struct TypedHandle<T: ResourceTag> {
    handle: Handle,
    _kind: PhantomData<T>,
}
impl<T: ResourceTag> TypedHandle<T> {
    pub const fn from_raw(raw: u64) -> Self {
        Self {
            handle: Handle::from_raw(raw),
            _kind: PhantomData,
        }
    }
    pub const fn raw(&self) -> u64 {
        self.handle.raw()
    }
    /// Use on EVERY operation. ObjectKey must be checked again under the
    /// future resource lock before accessing any device/VRAM/memory.
    pub fn authorize(
        &self,
        caller: u64,
        table: &HandleTable,
        objects: &ObjectRegistry,
        needed: Rights,
    ) -> Result<ObjectKey, CapError> {
        table.authorize_key(caller, objects, self.handle, T::KIND, needed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_resources_are_not_grants_and_require_live_rights() {
        let mut reg = ObjectRegistry::new();
        let device = reg.create(ObjectKind::Device).unwrap();
        let mut t = HandleTable::new(7);
        let h = t.grant(7, &reg, device, Rights::OBSERVE).unwrap();
        let observed = TypedHandle::<DeviceResource>::from_raw(h.raw());
        assert_eq!(observed.authorize(7, &t, &reg, Rights::OBSERVE), Ok(device));
        assert_eq!(
            observed.authorize(7, &t, &reg, Rights::CONTROL),
            Err(CapError::AccessDenied)
        );
        let miscast = TypedHandle::<SurfaceResource>::from_raw(h.raw());
        assert_eq!(
            miscast.authorize(7, &t, &reg, Rights::OBSERVE),
            Err(CapError::WrongType)
        );
        let fake = TypedHandle::<DeviceResource>::from_raw(h.raw().wrapping_add(1));
        assert_eq!(
            fake.authorize(7, &t, &reg, Rights::OBSERVE),
            Err(CapError::StaleHandle)
        );
        reg.revoke(device).unwrap();
        assert_eq!(
            observed.authorize(7, &t, &reg, Rights::OBSERVE),
            Err(CapError::Revoked)
        );
    }
}
