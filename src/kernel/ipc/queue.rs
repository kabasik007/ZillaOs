//! Deterministic bounded IPC queues. Not connected to a syscall or scheduler.
//! Calls must run under a future kernel synchronization/IRQ policy.
use crate::capabilities::{
    CapError, Delegation, HandleTable, ObjectId, ObjectRegistry,
};
use super::protocol::{
    MessageHeaderV1, MessageKind, ParsedMessage, MAX_MESSAGE_HANDLES, MAX_PAYLOAD_BYTES,
};

pub const QUEUE_DEPTH: usize = 8;
pub const MAX_PENDING_CALLS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueError {
    InvalidRequest, WrongPeer, NotFound, AlreadyExists, StillPending,
    Full, QueueFull, TimedOut, ServerGone, Canceled, StaleCall, BadCapability(CapError),
}
impl From<CapError> for QueueError {
    fn from(value: CapError) -> Self { Self::BadCapability(value) }
}

/// Created only by trusted kernel code using the current process identity.
/// Delegations reference kernel registry entries, never serialized pointers.
#[derive(Clone, Copy)]
pub struct Envelope {
    sender: u64,
    pub header: MessageHeaderV1,
    payload: [u8; MAX_PAYLOAD_BYTES],
    grants: [Option<Delegation>; MAX_MESSAGE_HANDLES],
    deadline_ms: Option<u64>,
}
impl Envelope {
    pub fn stage(
        sender: u64, parsed: &ParsedMessage<'_>, sender_table: &HandleTable,
        objects: &ObjectRegistry, deadline_ms: Option<u64>,
    ) -> Result<Self, QueueError> {
        if sender == 0 || sender != sender_table.owner() {
            return Err(QueueError::BadCapability(CapError::InvalidOwner));
        }
        let mut grants = [None; MAX_MESSAGE_HANDLES];
        for (i, slot) in grants.iter_mut().enumerate().take(parsed.header.handle_count) {
            let transfer = parsed.handle_request(i).ok_or(QueueError::InvalidRequest)?;
            *slot = Some(sender_table.prepare_transfer(
                sender, objects, transfer.handle, transfer.requested_rights,
            )?);
        }
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        payload[..parsed.payload.len()].copy_from_slice(parsed.payload);
        Ok(Self {
            sender, header: parsed.header, payload, grants, deadline_ms,
        })
    }
    pub const fn sender(&self) -> u64 { self.sender }
    pub fn payload(&self) -> &[u8] { &self.payload[..self.header.payload_len] }
    pub fn validate_transfers(&self, objects: &ObjectRegistry) -> Result<(), QueueError> {
        for grant in self.grants.iter().take(self.header.handle_count).flatten() {
            grant.validate(objects)?;
        }
        Ok(())
    }
    /// Atomic model delivery: receiver gets all requested handles or none.
    /// Must be executed under the future kernel endpoint/capability lock.
    pub fn deliver(
        &self, receiver: u64, receiver_table: &mut HandleTable,
        objects: &ObjectRegistry,
    ) -> Result<[Option<crate::capabilities::Handle>; MAX_MESSAGE_HANDLES], QueueError> {
        self.validate_transfers(objects)?;
        Ok(receiver_table.accept_many(receiver, objects, &self.grants)?)
    }
    /// Callers must validate this grant again during target HandleTable::accept.
    pub fn delegation(&self, index: usize, objects: &ObjectRegistry)
        -> Result<Option<Delegation>, QueueError>
    {
        if index >= self.header.handle_count { return Ok(None); }
        let grant = self.grants[index].ok_or(QueueError::InvalidRequest)?;
        grant.validate(objects)?;
        Ok(Some(grant))
    }
    fn expired(&self, now_ms: u64) -> bool {
        self.deadline_ms.is_some_and(|deadline| now_ms >= deadline)
    }
}

pub struct IpcQueue {
    server: ObjectId,
    slots: [Option<Envelope>; QUEUE_DEPTH],
    head: usize,
    len: usize,
    closed: bool,
    pub rejected_full: u64,
    pub expired: u64,
}
impl IpcQueue {
    pub const fn new(server: ObjectId) -> Self {
        Self {
            server, slots: [None; QUEUE_DEPTH], head: 0, len: 0,
            closed: false, rejected_full: 0, expired: 0,
        }
    }
    pub const fn server(&self) -> ObjectId { self.server }
    pub const fn len(&self) -> usize { self.len }
    pub const fn is_closed(&self) -> bool { self.closed }
    pub fn send(&mut self, msg: Envelope, now_ms: u64) -> Result<(), QueueError> {
        if self.closed { return Err(QueueError::ServerGone); }
        if msg.expired(now_ms) { return Err(QueueError::TimedOut); }
        if self.len == QUEUE_DEPTH {
            self.rejected_full = self.rejected_full.saturating_add(1);
            return Err(QueueError::QueueFull);
        }
        let tail = (self.head + self.len) % QUEUE_DEPTH;
        self.slots[tail] = Some(msg);
        self.len += 1;
        Ok(())
    }
    /// Atomic single-thread model: register correlation and enqueue or roll back
    /// the pending call on *any* queue failure. No SMP transaction yet.
    pub fn send_tracked(
        &mut self, calls: &mut CallTracker, msg: Envelope,
        deadline_ms: u64, now_ms: u64,
    ) -> Result<CallToken, QueueError> {
        if msg.header.kind != MessageKind::Request
            || msg.deadline_ms != Some(deadline_ms) {
            return Err(QueueError::InvalidRequest);
        }
        let client = msg.sender;
        let request_id = msg.header.request_id;
        let token = calls.start_token(client, self.server, request_id, deadline_ms, now_ms)?;
        if let Err(error) = self.send(msg, now_ms) {
            calls.rollback_start(token);
            return Err(error);
        }
        Ok(token)
    }
    /// Remove exiting sender's queued messages while preserving FIFO order.
    pub fn purge_sender(&mut self, sender: u64) -> usize {
        let original = self.len;
        let mut removed = 0;
        for _ in 0..original {
            let item = self.slots[self.head].take();
            self.head = (self.head + 1) % QUEUE_DEPTH;
            self.len -= 1;
            if let Some(message) = item {
                if message.sender == sender {
                    removed += 1;
                } else {
                    let tail = (self.head + self.len) % QUEUE_DEPTH;
                    self.slots[tail] = Some(message);
                    self.len += 1;
                }
            }
        }
        removed
    }
    /// Non-blocking model. Actual wait/wakeup requires a scheduler integration.
    pub fn recv(&mut self, now_ms: u64) -> Result<Option<Envelope>, QueueError> {
        if self.closed { return Err(QueueError::ServerGone); }
        while self.len != 0 {
            let result = self.slots[self.head].take();
            self.head = (self.head + 1) % QUEUE_DEPTH;
            self.len -= 1;
            if let Some(msg) = result {
                if msg.expired(now_ms) {
                    self.expired = self.expired.saturating_add(1);
                    continue;
                }
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }
    pub fn shutdown(&mut self, calls: &mut CallTracker) {
        self.closed = true;
        self.slots = [None; QUEUE_DEPTH];
        self.head = 0;
        self.len = 0;
        calls.server_gone(self.server);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus { Pending, Replied, Canceled, TimedOut, ServerGone }
/// Kernel-minted correlation. A recycled request ID cannot reuse a nonce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallToken {
    client: u64,
    request_id: u64,
    nonce: u64,
}
impl CallToken {
    pub const fn client(self) -> u64 { self.client }
    pub const fn request_id(self) -> u64 { self.request_id }
}
#[derive(Clone, Copy)]
struct Call {
    client: u64,
    server: ObjectId,
    request_id: u64,
    deadline_ms: u64,
    nonce: u64,
    status: CallStatus,
}
pub struct CallTracker {
    calls: [Option<Call>; MAX_PENDING_CALLS],
    next_nonce: u64,
}
impl Default for CallTracker { fn default() -> Self { Self::new() } }
impl CallTracker {
    pub const fn new() -> Self { Self { calls: [None; MAX_PENDING_CALLS], next_nonce: 1 } }
    pub fn count(&self) -> usize { self.calls.iter().flatten().count() }
    pub fn start(&mut self, client: u64, server: ObjectId, request_id: u64,
        deadline_ms: u64, now_ms: u64) -> Result<(), QueueError> {
        self.start_token(client, server, request_id, deadline_ms, now_ms).map(|_| ())
    }
    pub fn start_token(&mut self, client: u64, server: ObjectId, request_id: u64,
        deadline_ms: u64, now_ms: u64) -> Result<CallToken, QueueError>
    {
        if client == 0 || server.0 == 0 || request_id == 0 {
            return Err(QueueError::InvalidRequest);
        }
        if deadline_ms <= now_ms { return Err(QueueError::TimedOut); }
        if self.calls.iter().flatten().any(|c| c.client == client && c.request_id == request_id) {
            return Err(QueueError::AlreadyExists);
        }
        let slot = self.calls.iter_mut().find(|entry| entry.is_none())
            .ok_or(QueueError::Full)?;
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.checked_add(1).ok_or(QueueError::Full)?;
        *slot = Some(Call {
            client, server, request_id, deadline_ms, nonce, status: CallStatus::Pending,
        });
        Ok(CallToken { client, request_id, nonce })
    }
    /// Roll back only the specific still-pending call created by this send.
    fn rollback_start(&mut self, token: CallToken) {
        for entry in &mut self.calls {
            if entry.is_some_and(|call| call.client == token.client
                && call.request_id == token.request_id && call.nonce == token.nonce
                && call.status == CallStatus::Pending) {
                *entry = None;
                return;
            }
        }
    }
    /// Authenticated dispatcher must verify server endpoint capability too.
    /// This nonce also rejects delayed replies after request_id reuse.
    pub fn reply_token(&mut self, server: ObjectId, token: CallToken,
        now_ms: u64) -> Result<(), QueueError>
    {
        let call = self.find(token.client, token.request_id)?;
        if call.nonce != token.nonce { return Err(QueueError::StaleCall); }
        self.reply(server, token.client, token.request_id, now_ms)
    }
    /// Process exit removes both pending and completed correlations.
    pub fn client_exited(&mut self, client: u64) -> usize {
        let mut removed = 0;
        for entry in &mut self.calls {
            if entry.is_some_and(|call| call.client == client) {
                *entry = None;
                removed += 1;
            }
        }
        removed
    }
    fn find(&mut self, client: u64, request_id: u64) -> Result<&mut Call, QueueError> {
        self.calls.iter_mut().flatten()
            .find(|c| c.client == client && c.request_id == request_id)
            .ok_or(QueueError::NotFound)
    }
    pub fn reply(&mut self, server: ObjectId, client: u64, request_id: u64,
        now_ms: u64) -> Result<(), QueueError>
    {
        let call = self.find(client, request_id)?;
        if call.server != server { return Err(QueueError::WrongPeer); }
        if call.status != CallStatus::Pending { return Err(QueueError::Canceled); }
        if now_ms >= call.deadline_ms {
            call.status = CallStatus::TimedOut;
            return Err(QueueError::TimedOut);
        }
        call.status = CallStatus::Replied;
        Ok(())
    }
    pub fn cancel(&mut self, client: u64, request_id: u64) -> Result<(), QueueError> {
        let call = self.find(client, request_id)?;
        if call.status != CallStatus::Pending { return Err(QueueError::Canceled); }
        call.status = CallStatus::Canceled;
        Ok(())
    }
    pub fn expire(&mut self, now_ms: u64) {
        for call in self.calls.iter_mut().flatten() {
            if call.status == CallStatus::Pending && now_ms >= call.deadline_ms {
                call.status = CallStatus::TimedOut;
            }
        }
    }
    pub fn server_gone(&mut self, server: ObjectId) {
        for call in self.calls.iter_mut().flatten() {
            if call.server == server && call.status == CallStatus::Pending {
                call.status = CallStatus::ServerGone;
            }
        }
    }
    pub fn status(&self, client: u64, request_id: u64) -> Option<CallStatus> {
        self.calls.iter().flatten()
            .find(|c| c.client == client && c.request_id == request_id)
            .map(|c| c.status)
    }
    pub fn take_complete(&mut self, client: u64, request_id: u64)
        -> Result<CallStatus, QueueError>
    {
        let index = self.calls.iter()
            .position(|c| c.is_some_and(|v| v.client == client && v.request_id == request_id))
            .ok_or(QueueError::NotFound)?;
        let call = self.calls[index].ok_or(QueueError::NotFound)?;
        if call.status == CallStatus::Pending { return Err(QueueError::StillPending); }
        self.calls[index] = None;
        Ok(call.status)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeClock { now_ms: u64 }
impl FakeClock {
    pub const fn new(now_ms: u64) -> Self { Self { now_ms } }
    pub const fn now(self) -> u64 { self.now_ms }
    pub fn advance(&mut self, delta_ms: u64) -> Result<(), QueueError> {
        self.now_ms = self.now_ms.checked_add(delta_ms).ok_or(QueueError::InvalidRequest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{ObjectKind, Rights};
    use super::super::protocol::{parse, HEADER_BYTES, MAGIC, VERSION_MAJOR};
    fn parsed_header() -> [u8; HEADER_BYTES] {
        let mut b = [0; HEADER_BYTES];
        b[..4].copy_from_slice(&MAGIC.to_le_bytes());
        b[4..6].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        b[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        b[10..12].copy_from_slice(&1u16.to_le_bytes());
        b[12..16].copy_from_slice(&3u32.to_le_bytes());
        b[16..24].copy_from_slice(&4u64.to_le_bytes());
        b
    }
    fn envelope() -> Envelope {
        let b = parsed_header();
        let parsed = parse(&b).unwrap();
        Envelope::stage(2, &parsed, &HandleTable::new(2), &ObjectRegistry::new(), None).unwrap()
    }
    #[test]
    fn fifo_capacity_overflow_and_shutdown_are_bounded() {
        let server = ObjectId(8);
        let mut q = IpcQueue::new(server);
        for _ in 0..QUEUE_DEPTH { q.send(envelope(), 0).unwrap(); }
        assert_eq!(q.len(), QUEUE_DEPTH);
        assert_eq!(q.send(envelope(), 0), Err(QueueError::QueueFull));
        assert_eq!(q.rejected_full, 1);
        assert_eq!(q.recv(0).unwrap().unwrap().sender, 2);
        q.send(envelope(), 0).unwrap();
        let mut calls = CallTracker::new();
        calls.start(2, server, 4, 500, 0).unwrap();
        q.shutdown(&mut calls);
        assert_eq!(q.len(), 0);
        assert_eq!(calls.status(2, 4), Some(CallStatus::ServerGone));
        assert_eq!(q.send(envelope(), 1), Err(QueueError::ServerGone));
        assert!(matches!(q.recv(1), Err(QueueError::ServerGone)));
    }
    #[test]
    fn cancellation_deadlines_and_reply_peer_checks() {
        let mut clock = FakeClock::new(100);
        let mut calls = CallTracker::new();
        calls.start(2, ObjectId(5), 12, 120, clock.now()).unwrap();
        assert_eq!(calls.start(2, ObjectId(5), 12, 120, clock.now()), Err(QueueError::AlreadyExists));
        assert_eq!(calls.reply(ObjectId(6), 2, 12, 110), Err(QueueError::WrongPeer));
        clock.advance(20).unwrap();
        calls.expire(clock.now());
        assert_eq!(calls.status(2, 12), Some(CallStatus::TimedOut));
        assert_eq!(calls.take_complete(2, 12), Ok(CallStatus::TimedOut));
        calls.start(2, ObjectId(5), 12, 200, 130).unwrap();
        calls.cancel(2, 12).unwrap();
        assert_eq!(calls.reply(ObjectId(5), 2, 12, 140), Err(QueueError::Canceled));
        assert_eq!(calls.take_complete(2, 12), Ok(CallStatus::Canceled));
        calls.start(2, ObjectId(5), 12, 200, 130).unwrap();
        calls.reply(ObjectId(5), 2, 12, 140).unwrap();
        assert_eq!(calls.take_complete(2, 12), Ok(CallStatus::Replied));
    }
    #[test]
    fn queued_transfer_is_revalidated_after_global_revoke() {
        let mut objects = ObjectRegistry::new();
        let key = objects.create(ObjectKind::Endpoint).unwrap();
        let mut sender = HandleTable::new(2);
        let handle = sender.grant(2, &objects, key, Rights::SEND.union(Rights::TRANSFER)).unwrap();
        let mut wire = [0; HEADER_BYTES + 16];
        wire[..HEADER_BYTES].copy_from_slice(&parsed_header());
        wire[28..30].copy_from_slice(&1u16.to_le_bytes());
        wire[HEADER_BYTES..HEADER_BYTES + 8].copy_from_slice(&handle.raw().to_le_bytes());
        wire[HEADER_BYTES + 8..].copy_from_slice(&Rights::SEND.bits().to_le_bytes());
        let parsed = parse(&wire).unwrap();
        assert_eq!(Envelope::stage(99, &parsed, &sender, &objects, None).err(),
            Some(QueueError::BadCapability(CapError::InvalidOwner)));
        let env = Envelope::stage(2, &parsed, &sender, &objects, None).unwrap();
        assert_eq!(env.delegation(0, &objects).unwrap().unwrap().rights(), Rights::SEND);
        let mut q = IpcQueue::new(ObjectId(5));
        q.send(env, 0).unwrap();
        objects.revoke(key).unwrap();
        let msg = q.recv(0).unwrap().unwrap();
        assert_eq!(msg.validate_transfers(&objects), Err(QueueError::BadCapability(CapError::Revoked)));
    }
    #[test]
    fn queued_transfer_delivery_attenuates_then_revoke_invalidates_receiver() {
        let mut reg = ObjectRegistry::new();
        let key = reg.create(ObjectKind::Endpoint).unwrap();
        let mut from = HandleTable::new(2);
        let mut into = HandleTable::new(3);
        let source = from.grant(2, &reg, key, Rights::SEND.union(Rights::TRANSFER)).unwrap();
        let mut wire = [0u8; HEADER_BYTES + 16];
        wire[..HEADER_BYTES].copy_from_slice(&parsed_header());
        wire[28..30].copy_from_slice(&1u16.to_le_bytes());
        wire[HEADER_BYTES..HEADER_BYTES + 8].copy_from_slice(&source.raw().to_le_bytes());
        wire[HEADER_BYTES + 8..].copy_from_slice(&Rights::SEND.bits().to_le_bytes());
        let parsed = parse(&wire).unwrap();
        let staged = Envelope::stage(2, &parsed, &from, &reg, None).unwrap();
        let installed = staged.deliver(3, &mut into, &reg).unwrap()[0].unwrap();
        assert_eq!(into.authorize(3, &reg, installed, ObjectKind::Endpoint, Rights::SEND), Ok(key.id()));
        assert_eq!(into.authorize(3, &reg, installed, ObjectKind::Endpoint, Rights::CONTROL),
            Err(CapError::AccessDenied));
        reg.revoke(key).unwrap();
        assert_eq!(into.authorize(3, &reg, installed, ObjectKind::Endpoint, Rights::SEND),
            Err(CapError::Revoked));
        assert_eq!(staged.deliver(3, &mut into, &reg).err(),
            Some(QueueError::BadCapability(CapError::Revoked)));
        assert_eq!(into.count(), 1); // failed delivery did not install a second handle
    }
    #[test]
    fn enqueue_and_dequeue_expiry_is_deterministic() {
        let mut env = envelope();
        env.deadline_ms = Some(11);
        let mut q = IpcQueue::new(ObjectId(5));
        q.send(env, 10).unwrap();
        assert!(q.recv(11).unwrap().is_none());
        assert_eq!(q.expired, 1);
        assert_eq!(q.send(env, 11), Err(QueueError::TimedOut));
    }
    #[test]
    fn process_exit_purges_wrapped_queue_without_reordering_survivors() {
        let mut q = IpcQueue::new(ObjectId(8));
        for i in 0..QUEUE_DEPTH {
            let mut msg = envelope();
            msg.sender = if i % 2 == 0 { 2 } else { 3 };
            q.send(msg, 0).unwrap();
        }
        for _ in 0..3 { q.recv(0).unwrap(); }
        for i in 0..3 {
            let mut msg = envelope();
            msg.sender = if i == 0 { 2 } else { 3 };
            q.send(msg, 0).unwrap();
        }
        assert_eq!(q.purge_sender(2), 3);
        assert_eq!(q.len(), 5);
        for _ in 0..5 { assert_eq!(q.recv(0).unwrap().unwrap().sender(), 3); }
        assert!(q.recv(0).unwrap().is_none());
    }
    #[test]
    fn tracked_enqueue_rolls_back_and_old_reply_tokens_cannot_replay() {
        let mut q = IpcQueue::new(ObjectId(22));
        let mut calls = CallTracker::new();
        for _ in 0..QUEUE_DEPTH { q.send(envelope(), 0).unwrap(); }
        let mut staged = envelope();
        staged.deadline_ms = Some(200);
        assert_eq!(q.send_tracked(&mut calls, staged, 200, 1), Err(QueueError::QueueFull));
        assert_eq!(calls.count(), 0);
        q.recv(0).unwrap();
        let first = q.send_tracked(&mut calls, staged, 200, 1).unwrap();
        assert_eq!(calls.reply_token(ObjectId(23), first, 5), Err(QueueError::WrongPeer));
        assert_eq!(calls.reply_token(ObjectId(22), first, 5), Ok(()));
        assert_eq!(calls.reply_token(ObjectId(22), first, 6), Err(QueueError::Canceled));
        assert_eq!(calls.take_complete(2, 4), Ok(CallStatus::Replied));
        let newer = calls.start_token(2, ObjectId(22), 4, 220, 10).unwrap();
        assert_ne!(first, newer);
        assert_eq!(calls.reply_token(ObjectId(22), first, 11), Err(QueueError::StaleCall));
        assert_eq!(calls.status(2, 4), Some(CallStatus::Pending));
        assert_eq!(calls.reply_token(ObjectId(22), newer, 12), Ok(()));
        assert_eq!(calls.client_exited(2), 1);
        assert_eq!(calls.count(), 0);
    }
    #[test]
    fn pending_table_is_bounded_and_duplicate_ids_are_rejected() {
        let mut c = CallTracker::new();
        for request_id in 1..=MAX_PENDING_CALLS as u64 {
            c.start(1, ObjectId(7), request_id, 10, 0).unwrap();
        }
        assert_eq!(c.count(), MAX_PENDING_CALLS);
        assert_eq!(c.start(1, ObjectId(7), 99, 10, 0), Err(QueueError::Full));
        assert_eq!(c.take_complete(1, 1), Err(QueueError::StillPending));
        c.server_gone(ObjectId(7));
        assert_eq!(c.take_complete(1, 1), Ok(CallStatus::ServerGone));
    }
}