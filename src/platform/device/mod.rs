//! Single caller-owned registry; no competing global hardware inventories.
pub mod registry;

pub use registry::{
    DeviceId, DeviceRegistry, DeviceSnapshot, DeviceState, DeviceView, DriverId, DriverMatch,
    RegistryError,
};
