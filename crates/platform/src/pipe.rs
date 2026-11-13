use std::{
    mem::size_of,
    os::windows::io::OwnedHandle,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use crosshair_core::protocol::MAX_FRAME_BYTES;
use serde::{Serialize, de::DeserializeOwned};
use windows::{
    Win32::{
        Foundation::{
            ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
            LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            FreeSid,
            Isolation::DeriveAppContainerSidFromAppContainerName,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
            FILE_GENERIC_WRITE, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
            WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
            Threading::{
                CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
                WaitForSingleObject,
            },
        },
    },
    core::PCWSTR,
};

use crate::identity::{Identity, owned, raw, sid_string, wide};

const FRAME_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct StopSignal {
    event: Arc<OwnedHandle>,
}

impl StopSignal {
    pub fn new() -> Result<Self> {
        Ok(Self {
            event: Arc::new(unsafe { owned(CreateEventW(None, true, false, None)?) }),
        })
    }
    pub fn signal(&self) {
        unsafe {
            let _ = SetEvent(raw(&self.event));
        }
    }
    fn handle(&self) -> HANDLE {
        raw(&self.event)
    }
    fn is_set(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle(), 0) == WAIT_OBJECT_0 }
    }

    pub fn reset(&self) {
        unsafe {
            let _ = ResetEvent(self.handle());
        }
    }

    pub fn wait(&self, timeout: Option<Duration>) {
        let milliseconds = timeout.map_or(INFINITE, |time| {
            time.as_millis().min((u32::MAX - 1) as u128) as u32
        });
        unsafe {
            WaitForSingleObject(self.handle(), milliseconds);
        }
    }
}

#[derive(Clone)]
pub struct Pipe {
    handle: Arc<OwnedHandle>,
    stop: StopSignal,
}

impl Pipe {
    pub fn connect(name: &str, identity: &Identity, server_files: &[&str]) -> Result<Self> {
        let pipe = Self::open(name)?;
        identity.verify_peer(&pipe.server_identity()?, server_files)?;
        Ok(pipe)
    }

    pub fn connect_discord(index: u8, identity: &Identity) -> Result<Self> {
        ensure!(index < 10, "Invalid Discord pipe index");
        let pipe = Self::open(&format!(r"\\?\pipe\discord-ipc-{index}"))?;
        let peer = pipe.server_identity()?;
        ensure!(
            peer.user_sid == identity.user_sid
                && peer.logon_sid == identity.logon_sid
                && peer.session == identity.session,
            "Discord belongs to another Windows session"
        );
        Ok(pipe)
    }

    fn open(name: &str) -> Result<Self> {
        let name = wide(name);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?
        };
        Ok(Self {
            handle: Arc::new(unsafe { owned(handle) }),
            stop: StopSignal::new()?,
        })
    }

    fn server_identity(&self) -> Result<Identity> {
        let mut pid = 0;
        unsafe {
            GetNamedPipeServerProcessId(self.raw(), &mut pid)?;
        }
        Identity::process(pid)
    }

    pub fn raw(&self) -> HANDLE {
        raw(&self.handle)
    }

    pub fn cancel(&self) {
        self.stop.signal();
        unsafe {
            let _ = CancelIoEx(self.raw(), None);
            let _ = DisconnectNamedPipe(self.raw());
        }
    }

    pub fn read_message<T: DeserializeOwned>(&self) -> Result<T> {
        // Idle reads sleep indefinitely; the deadline begins with the first byte.
        let mut header = [0; 4];
        let count = self.read(&mut header, None)?;
        ensure!(count != 0, "The engine connection closed");
        let deadline = Instant::now() + FRAME_DEADLINE;
        self.read_exact(&mut header[count..], deadline)?;
        let length = u32::from_le_bytes(header) as usize;
        ensure!(
            length > 0 && length <= MAX_FRAME_BYTES,
            "Invalid control frame length"
        );
        let mut bytes = vec![0; length];
        self.read_exact(&mut bytes, deadline)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn write_message(&self, message: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(message)?;
        ensure!(
            bytes.len() <= MAX_FRAME_BYTES,
            "Control frame exceeds the limit"
        );
        let deadline = Instant::now() + FRAME_DEADLINE;
        self.write_all(&(bytes.len() as u32).to_le_bytes(), deadline)?;
        self.write_all(&bytes, deadline)
    }

    pub fn read_exact(&self, mut buffer: &mut [u8], deadline: Instant) -> Result<()> {
        while !buffer.is_empty() {
            let count = self.read(buffer, Some(deadline))?;
            ensure!(count > 0, "Incomplete pipe frame");
            buffer = &mut buffer[count..];
        }
        Ok(())
    }

    pub fn write_all(&self, mut buffer: &[u8], deadline: Instant) -> Result<()> {
        while !buffer.is_empty() {
            let count = self.write(buffer, deadline)?;
            ensure!(count > 0, "Pipe write made no progress");
            buffer = &buffer[count..];
        }
        Ok(())
    }

    pub fn read(&self, buffer: &mut [u8], deadline: Option<Instant>) -> Result<usize> {
        ensure!(!self.stop.is_set(), "Pipe connection closed");
        let operation = Operation::new()?;
        let mut overlap = operation.overlapped();
        let result = unsafe { ReadFile(self.raw(), Some(buffer), None, Some(&mut overlap)) };
        self.complete(result, &mut overlap, deadline)
    }

    /// Wait for bytes, a worker notification, or a deadline without polling.
    pub fn read_or_wake(
        &self,
        buffer: &mut [u8],
        deadline: Option<Instant>,
        wake: &StopSignal,
    ) -> Result<Option<usize>> {
        ensure!(!self.stop.is_set(), "Pipe connection closed");
        let operation = Operation::new()?;
        let mut overlap = operation.overlapped();
        let result = unsafe { ReadFile(self.raw(), Some(buffer), None, Some(&mut overlap)) };
        self.complete_or_wake(result, &mut overlap, deadline, wake.handle())
    }

    fn write(&self, buffer: &[u8], deadline: Instant) -> Result<usize> {
        ensure!(!self.stop.is_set(), "Pipe connection closed");
        let operation = Operation::new()?;
        let mut overlap = operation.overlapped();
        let result = unsafe { WriteFile(self.raw(), Some(buffer), None, Some(&mut overlap)) };
        self.complete(result, &mut overlap, Some(deadline))
    }

    fn complete(
        &self,
        result: windows::core::Result<()>,
        overlap: &mut OVERLAPPED,
        deadline: Option<Instant>,
    ) -> Result<usize> {
        self.complete_with_stop(result, overlap, deadline, self.stop.handle())
    }

    fn complete_with_stop(
        &self,
        result: windows::core::Result<()>,
        overlap: &mut OVERLAPPED,
        deadline: Option<Instant>,
        stop: HANDLE,
    ) -> Result<usize> {
        self.complete_or_wake(result, overlap, deadline, stop)?
            .ok_or_else(|| anyhow::anyhow!("Pipe operation timed out or was interrupted"))
    }

    fn complete_or_wake(
        &self,
        result: windows::core::Result<()>,
        overlap: &mut OVERLAPPED,
        deadline: Option<Instant>,
        wake: HANDLE,
    ) -> Result<Option<usize>> {
        if let Err(error) = result {
            if error.code() != ERROR_IO_PENDING.to_hresult() {
                return Err(error.into());
            }
            let milliseconds = deadline.map_or(INFINITE, |end| {
                end.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min((u32::MAX - 1) as u128) as u32
            });
            let result =
                unsafe { WaitForMultipleObjects(&[overlap.hEvent, wake], false, milliseconds) };
            if result != WAIT_OBJECT_0 {
                // Cancellation must complete before the stack OVERLAPPED and buffers go away.
                unsafe {
                    let _ = CancelIoEx(self.raw(), Some(overlap));
                    let mut count = 0;
                    match GetOverlappedResult(self.raw(), overlap, &mut count, true) {
                        // A simultaneous completion still owns these bytes; never discard them.
                        Ok(()) => return Ok(Some(count as usize)),
                        Err(error)
                            if error.code() == ERROR_OPERATION_ABORTED.to_hresult()
                                && (result == WAIT_TIMEOUT || result.0 == WAIT_OBJECT_0.0 + 1) =>
                        {
                            return Ok(None);
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        let mut count = 0;
        unsafe {
            GetOverlappedResult(self.raw(), overlap, &mut count, false)?;
        }
        Ok(Some(count as usize))
    }
}

struct Operation {
    event: OwnedHandle,
}
impl Operation {
    fn new() -> Result<Self> {
        Ok(Self {
            event: unsafe { owned(CreateEventW(None, true, false, None)?) },
        })
    }
    fn overlapped(&self) -> OVERLAPPED {
        OVERLAPPED {
            hEvent: raw(&self.event),
            ..Default::default()
        }
    }
}

pub struct Listener {
    name: String,
    identity: Identity,
    descriptor: SecurityDescriptor,
    pending: Pipe,
    allowed_clients: Vec<String>,
    stop: StopSignal,
}

impl Listener {
    pub fn bind(name: &str, identity: Identity, widget: bool) -> Result<Self> {
        let descriptor = SecurityDescriptor::new(&identity, widget)?;
        let pending = create(name, &descriptor, true)?;
        let allowed_clients = if widget {
            vec!["crosshair-gamebar.exe".into()]
        } else {
            vec!["crosshair-editor.exe".into(), "crosshair-engine.exe".into()]
        };
        Ok(Self {
            name: name.into(),
            identity,
            descriptor,
            pending,
            allowed_clients,
            stop: StopSignal::new()?,
        })
    }

    pub fn stop_signal(&self) -> StopSignal {
        self.stop.clone()
    }

    pub fn accept(&mut self) -> Result<Pipe> {
        ensure!(!self.stop.is_set(), "Listener stopped");
        let operation = Operation::new()?;
        let mut overlap = operation.overlapped();
        let result = unsafe { ConnectNamedPipe(self.pending.raw(), Some(&mut overlap)) };
        match result {
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
            result => {
                self.pending
                    .complete_with_stop(result, &mut overlap, None, self.stop.handle())?;
            }
        }
        // Keep an owned listening instance alive before returning the accepted one.
        let next = create(&self.name, &self.descriptor, false)?;
        let pipe = std::mem::replace(&mut self.pending, next);
        let mut pid = 0;
        unsafe {
            GetNamedPipeClientProcessId(pipe.raw(), &mut pid)?;
        }
        let peer = Identity::process(pid)?;
        let allowed = self
            .allowed_clients
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.identity.verify_peer(&peer, &allowed)?;
        Ok(pipe)
    }
}

struct SecurityDescriptor {
    value: PSECURITY_DESCRIPTOR,
}
impl SecurityDescriptor {
    fn new(identity: &Identity, widget: bool) -> Result<Self> {
        let mut sddl = format!("D:P(A;;GA;;;{})", identity.logon_sid);
        if widget {
            let family =
                wide(identity.package_family.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Game Bar requires the packaged application")
                })?);
            let sid =
                unsafe { DeriveAppContainerSidFromAppContainerName(PCWSTR(family.as_ptr()))? };
            let value = unsafe { sid_string(sid) };
            unsafe {
                FreeSid(sid);
            }
            // AppContainer access checks both ordinary and restricted package identities.
            sddl.push_str(&format!(
                "(A;;GRGW;;;WD)(A;;GRGW;;;{})S:(ML;;NW;;;LW)",
                value?
            ));
        }
        let sddl = wide(&sddl);
        let mut value = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut value,
                None,
            )?;
        }
        Ok(Self { value })
    }
}
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(Some(HLOCAL(self.value.0)));
        }
    }
}

fn create(name: &str, security: &SecurityDescriptor, first: bool) -> Result<Pipe> {
    let name = wide(name);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.value.0,
        bInheritHandle: false.into(),
    };
    let mut flags = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            8,
            64 * 1024,
            64 * 1024,
            5_000,
            Some(&attributes),
        )
    };
    ensure!(
        !handle.is_invalid(),
        "Could not create protected named pipe: {}",
        std::io::Error::last_os_error()
    );
    Ok(Pipe {
        handle: Arc::new(unsafe { owned(handle) }),
        stop: StopSignal::new()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::control_pipe_name;
    use crosshair_core::{
        Id,
        protocol::{Command, Request},
    };
    use std::{sync::mpsc, thread};

    #[test]
    fn authenticated_framing_and_idle_shutdown() -> Result<()> {
        let identity = Identity::current()?;
        let name = control_pipe_name(&Id::new_v4().to_string())?;
        let filename = identity
            .image
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (ready, received) = mpsc::channel();
        let server_name = name.clone();
        let server_file = filename.clone();
        let worker = thread::spawn(move || -> Result<()> {
            let mut listener = Listener::bind(&server_name, Identity::current()?, false)?;
            listener.allowed_clients = vec![server_file];
            ready.send(listener.stop_signal())?;
            let pipe = listener.accept()?;
            let request: Request = pipe.read_message()?;
            assert_eq!(request.id, 8);
            pipe.write_message(&request)?;
            // The main thread stops the next idle accept without a polling timer.
            assert!(listener.accept().is_err());
            Ok(())
        });
        let stop = received.recv_timeout(Duration::from_secs(5))?;
        let pipe = Pipe::connect(&name, &identity, &[&filename])?;
        pipe.write_message(&Request::new(8, Command::Subscribe))?;
        let reply: Request = pipe.read_message()?;
        assert_eq!(reply.id, 8);
        stop.signal();
        worker.join().expect("pipe worker panicked")?;
        Ok(())
    }

    #[test]
    fn first_instance_cannot_be_taken_over() -> Result<()> {
        let name = control_pipe_name(&Id::new_v4().to_string())?;
        let _first = Listener::bind(&name, Identity::current()?, false)?;
        assert!(Listener::bind(&name, Identity::current()?, false).is_err());
        Ok(())
    }
}
