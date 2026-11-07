//! Trusted ProcessTable <-> IPC/Capability lifecycle adapter (model only).
//! No user pointers, syscalls, scheduler threads or fake PID registry.
use crate::capabilities::{
    CapError, Handle, ObjectKey, ObjectKind, ObjectRegistry, Rights,
};
use crate::capabilities::process_binding::{
    BindingError, ProcessHandleBook, current_running,
};
use crate::process::{Pid, ProcessState, ProcessTable};
use super::protocol::ParsedMessage;
use super::queue::{CallToken, CallTracker, Envelope, IpcQueue, QueueError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    ProcessNotExited,
    WrongEndpoint,
    WrongEndpointOwner,
    SenderExited,
    Binding(BindingError),
    Queue(QueueError),
    Capability(CapError),
}
impl From<BindingError> for LifecycleError {
    fn from(value: BindingError) -> Self { Self::Binding(value) }
}
impl From<QueueError> for LifecycleError {
    fn from(value: QueueError) -> Self { Self::Queue(value) }
}
impl From<CapError> for LifecycleError {
    fn from(value: CapError) -> Self { Self::Capability(value) }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitCleanup {
    pub closed_handles: usize,
    pub revoked_objects: usize,
    pub purged_messages: usize,
    pub closed_endpoints: usize,
    pub forgotten_client_calls: usize,
}

/// Explicit trusted issuer operation. No capability-label string is a grant.
pub fn register_owned_object(
    processes: &ProcessTable, book: &ProcessHandleBook,
    registry: &mut ObjectRegistry, owner: Pid, kind: ObjectKind,
) -> Result<ObjectKey, LifecycleError> {
    let record = processes.process(owner).ok_or(BindingError::MissingProcess)?;
    if record.state == ProcessState::Exited { return Err(BindingError::ProcessExited.into()); }
    book.by_pid(owner)?;
    Ok(registry.create_owned(kind, owner.0)?)
}

/// Sender identity is extracted from canonical ProcessTable Running state,
/// never from payload, user registers, wire header or service manifest.
pub fn send_from_running(
    processes: &ProcessTable, book: &ProcessHandleBook, registry: &ObjectRegistry,
    queue: &mut IpcQueue, calls: &mut CallTracker, endpoint: Handle,
    parsed: &ParsedMessage<'_>, deadline_ms: u64, now_ms: u64,
) -> Result<CallToken, LifecycleError> {
    let (table, pid) = book.running_table(processes)?;
    let endpoint_key = table.authorize_key(
        pid.0, registry, endpoint, ObjectKind::Endpoint, Rights::SEND,
    )?;
    if endpoint_key.id() != queue.server() { return Err(LifecycleError::WrongEndpoint); }
    registry.validate(endpoint_key)?;
    let msg = Envelope::stage(pid.0, parsed, table, registry, Some(deadline_ms))?;
    Ok(queue.send_tracked(calls, msg, deadline_ms, now_ms)?)
}

/// Only endpoint owner may receive. A transferred RECEIVE right alone does not
/// authorize claiming a different server's endpoint identity.
pub fn receive_for_running(
    processes: &ProcessTable, book: &ProcessHandleBook, registry: &ObjectRegistry,
    queue: &mut IpcQueue, endpoint: Handle, now_ms: u64,
) -> Result<Option<Envelope>, LifecycleError> {
    let (table, pid) = book.running_table(processes)?;
    let key = table.authorize_key(pid.0, registry, endpoint, ObjectKind::Endpoint, Rights::RECEIVE)?;
    if key.id() != queue.server() { return Err(LifecycleError::WrongEndpoint); }
    if registry.owner_of(key)? != Some(pid.0) { return Err(LifecycleError::WrongEndpointOwner); }
    loop {
        let Some(msg) = queue.recv(now_ms)? else { return Ok(None); };
        let sender_alive = processes.process(Pid(msg.sender()))
            .is_some_and(|p| p.state != ProcessState::Exited);
        if !sender_alive { continue; } // dropped stale send, no transfer
        msg.validate_transfers(registry)?;
        return Ok(Some(msg));
    }
}

/// Incoming handle grant installation is all-or-nothing at the model layer.
/// Kernel must invoke only on an authenticated received envelope.
pub fn deliver_to_running(
    processes: &ProcessTable, book: &mut ProcessHandleBook, registry: &ObjectRegistry,
    msg: &Envelope,
) -> Result<[Option<Handle>; super::protocol::MAX_MESSAGE_HANDLES], LifecycleError> {
    let pid = current_running(processes)?.pid();
    if !processes.process(Pid(msg.sender())).is_some_and(|p| p.state != ProcessState::Exited) {
        return Err(LifecycleError::SenderExited);
    }
    let receiver = book.by_pid_mut(pid)?;
    Ok(msg.deliver(pid.0, receiver, registry)?)
}

/// Reply must be from the authoritative endpoint owner holding RECEIVE.
/// Token nonce rejects replay after request_id reuse.
pub fn reply_from_running(
    processes: &ProcessTable, book: &ProcessHandleBook, registry: &ObjectRegistry,
    queue: &IpcQueue, calls: &mut CallTracker, endpoint: Handle,
    token: CallToken, now_ms: u64,
) -> Result<(), LifecycleError> {
    if queue.is_closed() { return Err(QueueError::ServerGone.into()); }
    let (table, pid) = book.running_table(processes)?;
    let key = table.authorize_key(pid.0, registry, endpoint, ObjectKind::Endpoint, Rights::RECEIVE)?;
    if key.id() != queue.server() { return Err(LifecycleError::WrongEndpoint); }
    if registry.owner_of(key)? != Some(pid.0) { return Err(LifecycleError::WrongEndpointOwner); }
    Ok(calls.reply_token(key.id(), token, now_ms)?)
}

/// This callback must be invoked by canonical ProcessManager after it records
/// Exited and BEFORE it reaps the process. Never accept a wire "process_exit".
///
/// Requires exclusive access to all tables, queues and object registry.
/// Atomicity against IRQ/SMP races requires a future common kernel lock.
pub fn process_exit_cleanup(
    processes: &ProcessTable, pid: Pid, book: &mut ProcessHandleBook,
    registry: &mut ObjectRegistry, calls: &mut CallTracker,
    queues: &mut [&mut IpcQueue],
) -> Result<ExitCleanup, LifecycleError> {
    let process = processes.process(pid).ok_or(BindingError::MissingProcess)?;
    if process.state != ProcessState::Exited { return Err(LifecycleError::ProcessNotExited); }
    book.by_pid(pid)?; // preflight ensures no half-teardown from missing owner
    let mut cleanup = ExitCleanup::default();
    for queue in queues {
        if registry.owner_of_id(queue.server()) == Some(pid.0) {
            cleanup.purged_messages += queue.len();
            queue.shutdown(calls); // waiting clients become ServerGone
            cleanup.closed_endpoints += 1;
        } else {
            cleanup.purged_messages += queue.purge_sender(pid.0);
        }
    }
    cleanup.forgotten_client_calls = calls.client_exited(pid.0);
    cleanup.revoked_objects = registry.revoke_owned(pid.0);
    cleanup.closed_handles = book.cleanup_exited(processes, pid)?;
    Ok(cleanup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{parse, HEADER_BYTES, MAGIC, VERSION_MAJOR};
    use crate::ipc::queue::{CallStatus, QUEUE_DEPTH};
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};

    fn ready(processes: &mut ProcessTable, name: &'static str) -> Pid {
        let pid = processes.create(name, 0x400000, 0x800000, 8).unwrap();
        processes.map_region(pid, UserRegion::checked(0x400000, PAGE_SIZE,
            Permissions::READ_EXECUTE).unwrap()).unwrap();
        processes.prepare(pid).unwrap();
        pid
    }
    fn wire_request(forged_sender: u64) -> [u8; HEADER_BYTES + 8] {
        let mut b = [0u8; HEADER_BYTES + 8];
        b[..4].copy_from_slice(&MAGIC.to_le_bytes());
        b[4..6].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        b[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        b[10..12].copy_from_slice(&1u16.to_le_bytes());
        b[12..16].copy_from_slice(&7u32.to_le_bytes());
        b[16..24].copy_from_slice(&1u64.to_le_bytes());
        b[24..28].copy_from_slice(&8u32.to_le_bytes());
        b[HEADER_BYTES..].copy_from_slice(&forged_sender.to_le_bytes());
        b
    }
    #[test]
    fn forged_sender_in_payload_cannot_change_trusted_pid() {
        let mut processes = ProcessTable::new();
        let client = ready(&mut processes, "client");
        let server = ready(&mut processes, "server");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, client).unwrap();
        book.bind(&processes, server).unwrap();
        let mut objects = ObjectRegistry::new();
        let endpoint = register_owned_object(&processes, &book, &mut objects,
            server, ObjectKind::Endpoint).unwrap();
        let client_handle = book.kernel_grant(&processes, client, &objects, endpoint,
            Rights::SEND).unwrap();
        let mut queue = IpcQueue::new(endpoint.id());
        let mut calls = CallTracker::new();
        processes.mark_running(client).unwrap();
        let bytes = wire_request(server.0); // forged sender in payload
        let parsed = parse(&bytes).unwrap();
        let token = send_from_running(&processes, &book, &objects,
            &mut queue, &mut calls, client_handle, &parsed, 100, 1).unwrap();
        assert_eq!(token.client(), client.0);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.recv(1).unwrap().unwrap().sender(), client.0);
        assert_eq!(calls.status(client.0, 1), Some(CallStatus::Pending));
    }
    #[test]
    fn crashed_server_revokes_endpoint_and_unblocks_clients() {
        let mut processes = ProcessTable::new();
        let client = ready(&mut processes, "client");
        let server = ready(&mut processes, "server");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, client).unwrap();
        book.bind(&processes, server).unwrap();
        let mut objects = ObjectRegistry::new();
        let endpoint = register_owned_object(&processes, &book, &mut objects,
            server, ObjectKind::Endpoint).unwrap();
        let client_handle = book.kernel_grant(&processes, client, &objects, endpoint,
            Rights::SEND).unwrap();
        book.kernel_grant(&processes, server, &objects, endpoint, Rights::RECEIVE).unwrap();
        let mut queue = IpcQueue::new(endpoint.id());
        let mut calls = CallTracker::new();
        processes.mark_running(client).unwrap();
        let wire = wire_request(99999999);
        let parsed = parse(&wire).unwrap();
        let token = send_from_running(&processes, &book, &objects,
            &mut queue, &mut calls, client_handle, &parsed, 50, 0).unwrap();
        processes.kill(server).unwrap(); // canonical ProcessTable exit event
        let done = process_exit_cleanup(&processes, server, &mut book,
            &mut objects, &mut calls, &mut [&mut queue]).unwrap();
        assert_eq!(done.closed_endpoints, 1);
        assert_eq!(done.revoked_objects, 1);
        assert_eq!(done.purged_messages, 1);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_closed());
        assert_eq!(calls.status(client.0, token.request_id()), Some(CallStatus::ServerGone));
        assert_eq!(objects.validate(endpoint), Err(CapError::Revoked));
        assert_eq!(send_from_running(&processes, &book, &objects, &mut queue,
            &mut calls, client_handle, &parsed, 100, 1), Err(LifecycleError::Capability(CapError::Revoked)));
    }
    #[test]
    fn client_exit_purges_messages_and_pending_calls_without_reaping_process_table() {
        let mut processes = ProcessTable::new();
        let client = ready(&mut processes, "client");
        let server = ready(&mut processes, "server");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, client).unwrap();
        book.bind(&processes, server).unwrap();
        let mut objects = ObjectRegistry::new();
        let endpoint = register_owned_object(&processes, &book, &mut objects,
            server, ObjectKind::Endpoint).unwrap();
        let client_handle = book.kernel_grant(&processes, client, &objects,
            endpoint, Rights::SEND).unwrap();
        let mut queue = IpcQueue::new(endpoint.id());
        let mut calls = CallTracker::new();
        processes.mark_running(client).unwrap();
        let wire = wire_request(server.0);
        let parsed = parse(&wire).unwrap();
        send_from_running(&processes, &book, &objects, &mut queue,
            &mut calls, client_handle, &parsed, 500, 1).unwrap();
        assert_eq!(process_exit_cleanup(&processes, client, &mut book,
            &mut objects, &mut calls, &mut [&mut queue]),
            Err(LifecycleError::ProcessNotExited));
        processes.kill(client).unwrap();
        let done = process_exit_cleanup(&processes, client, &mut book,
            &mut objects, &mut calls, &mut [&mut queue]).unwrap();
        assert_eq!(done.closed_endpoints, 0);
        assert_eq!(done.closed_handles, 1);
        assert_eq!(done.purged_messages, 1);
        assert_eq!(done.forgotten_client_calls, 1);
        assert_eq!(queue.len(), 0);
        assert_eq!(calls.count(), 0);
        assert_eq!(objects.validate(endpoint), Ok(())); // server survives
        assert!(processes.process(client).is_some()); // not a second process registry
    }
    #[test]
    fn denied_undelegated_endpoint_does_not_mutate_queue_or_calls() {
        let mut processes = ProcessTable::new();
        let client = ready(&mut processes, "client");
        let server = ready(&mut processes, "server");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, client).unwrap();
        book.bind(&processes, server).unwrap();
        let mut objects = ObjectRegistry::new();
        let endpoint = register_owned_object(&processes, &book, &mut objects,
            server, ObjectKind::Endpoint).unwrap();
        let mut queue = IpcQueue::new(endpoint.id());
        let mut calls = CallTracker::new();
        processes.mark_running(client).unwrap();
        let wire = wire_request(server.0);
        let parsed = parse(&wire).unwrap();
        assert_eq!(send_from_running(&processes, &book, &objects, &mut queue,
            &mut calls, Handle::from_raw(0x100000001), &parsed, 40, 0),
            Err(LifecycleError::Capability(CapError::StaleHandle)));
        assert_eq!(queue.len(), 0);
        assert_eq!(calls.count(), 0);
        // saturate queue to ensure rollback does not create orphan call
        let valid = book.kernel_grant(&processes, client, &objects, endpoint,
            Rights::SEND).unwrap();
        for _ in 0..QUEUE_DEPTH {
            // duplicate request IDs are intentionally NOT recorded via raw queue.
            let env = Envelope::stage(client.0, &parsed, book.by_pid(client).unwrap(),
                &objects, Some(40)).unwrap();
            queue.send(env, 0).unwrap();
        }
        assert_eq!(send_from_running(&processes, &book, &objects, &mut queue,
            &mut calls, valid, &parsed, 40, 0),
            Err(LifecycleError::Queue(QueueError::QueueFull)));
        assert_eq!(calls.count(), 0);
    }
    #[test]
    fn restart_creates_new_endpoint_id_and_old_handle_stays_revoked() {
        let mut processes = ProcessTable::new();
        let client = ready(&mut processes, "client");
        let old_server = ready(&mut processes, "old-server");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, client).unwrap();
        book.bind(&processes, old_server).unwrap();
        let mut objects = ObjectRegistry::new();
        let old_key = register_owned_object(&processes, &book, &mut objects,
            old_server, ObjectKind::Endpoint).unwrap();
        let old_handle = book.kernel_grant(&processes, client, &objects, old_key,
            Rights::SEND).unwrap();
        let mut queue = IpcQueue::new(old_key.id());
        let mut calls = CallTracker::new();
        processes.mark_running(client).unwrap();
        processes.kill(old_server).unwrap();
        process_exit_cleanup(&processes, old_server, &mut book, &mut objects,
            &mut calls, &mut [&mut queue]).unwrap();
        let fresh = ready(&mut processes, "new-server");
        assert_ne!(fresh, old_server);
        book.bind(&processes, fresh).unwrap();
        let fresh_key = register_owned_object(&processes, &book, &mut objects,
            fresh, ObjectKind::Endpoint).unwrap();
        assert_ne!(old_key.id(), fresh_key.id());
        assert_eq!(book.authorize_running(&processes, &objects, old_handle,
            ObjectKind::Endpoint, Rights::SEND),
            Err(BindingError::Capability(CapError::Revoked)));
        let new_handle = book.kernel_grant(&processes, client, &objects, fresh_key,
            Rights::SEND).unwrap();
        assert_eq!(book.authorize_running(&processes, &objects, new_handle,
            ObjectKind::Endpoint, Rights::SEND).unwrap(), fresh_key);
    }
    #[test]
    fn process_exit_reclaims_capabilities_even_with_unreaped_child_tid() {
        use crate::process::thread::{ThreadError, ThreadTable};
        let mut processes = ProcessTable::new();
        let parent = ready(&mut processes, "parent");
        let mut threads = ThreadTable::new(); // authoritative thread model, not ours
        let tid = threads.create(&processes, parent, 0x400000, 0x800000, 1).unwrap();
        threads.prepare(&processes, tid).unwrap();
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, parent).unwrap();
        let mut registry = ObjectRegistry::new();
        let owned = register_owned_object(&processes, &book, &mut registry,
            parent, ObjectKind::Surface).unwrap();
        book.kernel_grant(&processes, parent, &registry, owned, Rights::OBSERVE).unwrap();
        processes.kill(parent).unwrap();
        let mut calls = CallTracker::new();
        let mut no_queues: [&mut IpcQueue; 0] = [];
        let cleaned = process_exit_cleanup(&processes, parent, &mut book,
            &mut registry, &mut calls, &mut no_queues).unwrap();
        assert_eq!(cleaned.closed_handles, 1);
        assert_eq!(cleaned.revoked_objects, 1);
        assert_eq!(registry.validate(owned), Err(CapError::Revoked));
        assert_eq!(threads.get(tid).unwrap().pid, parent);
        assert_eq!(threads.create(&processes, parent, 0x400000, 0x800000, 1),
            Err(ThreadError::ParentMissing));
        threads.exit(tid).unwrap();
        threads.reap(tid).unwrap(); // ProcessManager remains responsible for PID reap
    }
    #[test]
    fn already_received_delegation_cannot_outlive_sender_exit() {
        use crate::ipc::protocol::HANDLE_DESCRIPTOR_BYTES;
        let mut processes = ProcessTable::new();
        let sender = ready(&mut processes, "sender");
        let receiver = ready(&mut processes, "receiver");
        let mut book = ProcessHandleBook::new();
        book.bind(&processes, sender).unwrap();
        book.bind(&processes, receiver).unwrap();
        let mut registry = ObjectRegistry::new();
        let key = register_owned_object(&processes, &book, &mut registry,
            sender, ObjectKind::Device).unwrap();
        let source = book.kernel_grant(&processes, sender, &registry, key,
            Rights::TRANSFER.union(Rights::OBSERVE)).unwrap();
        let mut wire = [0u8; HEADER_BYTES + HANDLE_DESCRIPTOR_BYTES];
        let request = wire_request(777);
        wire[..HEADER_BYTES].copy_from_slice(&request[..HEADER_BYTES]);
        wire[24..28].copy_from_slice(&0u32.to_le_bytes());
        wire[28..30].copy_from_slice(&1u16.to_le_bytes());
        wire[HEADER_BYTES..HEADER_BYTES+8].copy_from_slice(&source.raw().to_le_bytes());
        wire[HEADER_BYTES+8..].copy_from_slice(&Rights::OBSERVE.bits().to_le_bytes());
        let parsed = parse(&wire).unwrap();
        processes.mark_running(sender).unwrap();
        let received = Envelope::stage(sender.0, &parsed, book.by_pid(sender).unwrap(),
            &registry, Some(100)).unwrap();
        processes.kill(sender).unwrap();
        let mut calls = CallTracker::new();
        let mut none: [&mut IpcQueue; 0] = [];
        process_exit_cleanup(&processes, sender, &mut book, &mut registry,
            &mut calls, &mut none).unwrap();
        processes.mark_running(receiver).unwrap();
        assert!(matches!(deliver_to_running(&processes, &mut book, &registry, &received),
            Err(LifecycleError::SenderExited)));
        assert_eq!(book.by_pid(receiver).unwrap().count(), 0);
    }

}
