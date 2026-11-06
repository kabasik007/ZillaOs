//! Validation-only Zilla native ABI v0, not an installed trap handler.
//! Never dereference user pointers without a future copy-from-user primitive.
use super::address_space::Access;
use super::Process;

pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 2;
pub const STDOUT: u64 = 1;
pub const MAX_WRITE: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub number: u64,
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Write { address: u64, len: u64 },
    Exit { code: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidHandle,
    InvalidLength,
    InvalidUserBuffer,
}

pub fn validate(process: &Process, request: Request) -> Result<Action, Error> {
    match request.number {
        SYS_WRITE => {
            if request.arg0 != STDOUT { return Err(Error::InvalidHandle) }
            if request.arg2 == 0 || request.arg2 > MAX_WRITE { return Err(Error::InvalidLength) }
            if !process.address_space.allows(request.arg1, request.arg2, Access::Read) {
                return Err(Error::InvalidUserBuffer);
            }
            Ok(Action::Write { address: request.arg1, len: request.arg2 })
        }
        SYS_EXIT => Ok(Action::Exit { code: request.arg0 as i32 }),
        _ => Err(Error::Unsupported),
    }
}
