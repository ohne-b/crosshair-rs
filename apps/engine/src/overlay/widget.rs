pub mod presence;

use std::{
    os::windows::io::OwnedHandle,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use crosshair_core::protocol::WidgetStatus;
use crosshair_platform::{
    identity::{Identity, raw},
    pipe::{Listener, Pipe, StopSignal},
};
use crosshair_renderer::Raster;
use windows::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};

enum LastHost {
    NeverConnected,
    Observed(Option<OwnedHandle>),
}

impl LastHost {
    fn gone(&self) -> bool {
        match self {
            Self::NeverConnected => true,
            Self::Observed(Some(process)) => unsafe {
                WaitForSingleObject(raw(process), 0) == WAIT_OBJECT_0
            },
            Self::Observed(None) => false,
        }
    }
}

#[derive(Clone)]
struct Frame {
    generation: u64,
    raster: Option<Arc<Raster>>,
    origin: [i32; 2],
    center: bool,
    close: bool,
    prepare: bool,
}

#[derive(Default)]
pub struct Status {
    pub generation: u64,
    pub prepared: bool,
    pub presented: bool,
    pub widget: WidgetStatus,
}

struct Shared {
    frame: Mutex<Frame>,
    changed: Condvar,
    stopped: AtomicBool,
    connected: AtomicBool,
    last_host: Mutex<LastHost>,
    pipe: Mutex<Option<Pipe>>,
}

pub struct Widget {
    shared: Arc<Shared>,
    stop: StopSignal,
    worker: Option<JoinHandle<()>>,
}

impl Widget {
    pub fn start(
        identity: Identity,
        notify: impl Fn(Status) + Send + Sync + 'static,
    ) -> Result<Self> {
        ensure!(
            identity.package.is_some(),
            "Game Bar requires the installed application"
        );
        let shared = Arc::new(Shared {
            frame: Mutex::new(Frame {
                generation: 1,
                raster: None,
                origin: [0, 0],
                center: false,
                close: false,
                prepare: false,
            }),
            changed: Condvar::new(),
            stopped: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            last_host: Mutex::new(LastHost::NeverConnected),
            pipe: Mutex::new(None),
        });
        let (started, ready) = mpsc::sync_channel(1);
        let thread_shared = shared.clone();
        let notify = Arc::new(notify);
        let worker = thread::Builder::new()
            .name("widget connection".into())
            .spawn(move || {
                let mut listener = match Listener::bind_widget(identity.clone()) {
                    Ok(listener) => listener,
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                if started.send(Ok(listener.stop_signal())).is_err() {
                    return;
                }
                while !thread_shared.stopped.load(Ordering::Acquire) {
                    let pipe = match listener.accept() {
                        Ok(pipe) => pipe,
                        Err(_) => continue,
                    };
                    *thread_shared.pipe.lock().unwrap() = Some(pipe.clone());
                    thread_shared.connected.store(true, Ordering::Release);
                    *thread_shared.last_host.lock().unwrap() =
                        LastHost::Observed(pipe.client_exit_handle().ok());
                    let reading = pipe.clone();
                    let callback = notify.clone();
                    let disconnected = Arc::new(AtomicBool::new(false));
                    let read_disconnected = disconnected.clone();
                    let read_shared = thread_shared.clone();
                    let session = identity.session;
                    let reader =
                        thread::Builder::new()
                            .name("widget status".into())
                            .spawn(move || {
                                while let Ok(status) = read_status(&reading, session) {
                                    callback(status);
                                }
                                {
                                    let _frame = read_shared.frame.lock().unwrap();
                                    read_disconnected.store(true, Ordering::Release);
                                    read_shared.connected.store(false, Ordering::Release);
                                    read_shared.changed.notify_all();
                                }
                                reading.cancel();
                            });
                    if reader.is_err() {
                        thread_shared.connected.store(false, Ordering::Release);
                        pipe.cancel();
                        continue;
                    }
                    let mut sent = 0;
                    loop {
                        let frame = {
                            let mut frame = thread_shared.frame.lock().unwrap();
                            while frame.generation == sent
                                && !thread_shared.stopped.load(Ordering::Acquire)
                                && !disconnected.load(Ordering::Acquire)
                            {
                                frame = thread_shared.changed.wait(frame).unwrap();
                            }
                            if thread_shared.stopped.load(Ordering::Acquire)
                                || disconnected.load(Ordering::Acquire)
                            {
                                break;
                            }
                            frame.clone()
                        };
                        let deadline = Instant::now() + Duration::from_secs(5);
                        let sent_result =
                            pipe.write_all(&header(&frame), deadline).and_then(|_| {
                                if let Some(raster) = &frame.raster {
                                    pipe.write_all(&raster.to_bgra(), deadline)
                                } else {
                                    Ok(())
                                }
                            });
                        if sent_result.is_err() {
                            break;
                        }
                        sent = frame.generation;
                    }
                    pipe.cancel();
                    *thread_shared.pipe.lock().unwrap() = None;
                    if let Ok(reader) = reader {
                        let _ = reader.join();
                    }
                    notify(Status {
                        widget: WidgetStatus {
                            installed: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
            })?;
        let stop = ready.recv()??;
        Ok(Self {
            shared,
            stop,
            worker: Some(worker),
        })
    }

    pub fn present(&self, raster: Option<Arc<Raster>>, origin: [i32; 2], center: bool) -> u64 {
        self.set_frame(raster, origin, center, false)
    }

    pub fn prepare(&self, raster: Arc<Raster>, origin: [i32; 2]) -> u64 {
        self.set_frame(Some(raster), origin, true, true)
    }

    fn set_frame(
        &self,
        raster: Option<Arc<Raster>>,
        origin: [i32; 2],
        center: bool,
        prepare: bool,
    ) -> u64 {
        let mut frame = self.shared.frame.lock().unwrap();
        let same = match (&frame.raster, &raster) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same && frame.origin == origin && frame.prepare == prepare && !center {
            return frame.generation;
        }
        frame.generation = frame.generation.saturating_add(1);
        frame.raster = raster;
        frame.origin = origin;
        frame.center = center;
        frame.prepare = prepare;
        self.shared.changed.notify_all();
        frame.generation
    }

    pub fn close(&self) {
        let mut frame = self.shared.frame.lock().unwrap();
        frame.generation = frame.generation.saturating_add(1);
        frame.raster = None;
        frame.close = true;
        frame.prepare = false;
        frame.center = false;
        self.shared.changed.notify_all();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.shared.connected.load(Ordering::Acquire) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let (next, timeout) = self.shared.changed.wait_timeout(frame, remaining).unwrap();
            frame = next;
            if timeout.timed_out() {
                break;
            }
        }
    }

    pub fn disconnect(&self) {
        if let Some(pipe) = self.shared.pipe.lock().unwrap().as_ref() {
            pipe.cancel();
        }
    }

    pub fn host_gone(&self) -> bool {
        self.shared.last_host.lock().unwrap().gone()
    }
}

impl Drop for Widget {
    fn drop(&mut self) {
        {
            let _frame = self.shared.frame.lock().unwrap();
            self.shared.stopped.store(true, Ordering::Release);
            self.shared.changed.notify_all();
        }
        self.stop.signal();
        if let Some(pipe) = self.shared.pipe.lock().unwrap().as_ref() {
            pipe.cancel();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn header(frame: &Frame) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"CRSF");
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&40u16.to_le_bytes());
    bytes[8..16].copy_from_slice(&frame.generation.to_le_bytes());
    let mut flags = (u32::from(frame.center) * 2)
        | (u32::from(frame.close) * 4)
        | (u32::from(frame.prepare) * 8);
    if let Some(raster) = &frame.raster {
        flags |= 1;
        bytes[16..20].copy_from_slice(&raster.width.to_le_bytes());
        bytes[20..24].copy_from_slice(&raster.height.to_le_bytes());
        bytes[24..28].copy_from_slice(&(raster.width * 4).to_le_bytes());
        bytes[28..32].copy_from_slice(&(frame.origin[0] + raster.width as i32 / 2).to_le_bytes());
        bytes[32..36].copy_from_slice(&(frame.origin[1] + raster.height as i32 / 2).to_le_bytes());
    }
    bytes[36..40].copy_from_slice(&flags.to_le_bytes());
    bytes
}

fn read_status(pipe: &Pipe, session: u32) -> Result<Status> {
    let mut bytes = [0; 64];
    let count = pipe.read(&mut bytes[..1], None)?;
    ensure!(count == 1, "Widget disconnected");
    pipe.read_exact(&mut bytes[1..], Instant::now() + Duration::from_secs(5))?;
    decode_status(&bytes, session)
}

fn decode_status(bytes: &[u8; 64], session: u32) -> Result<Status> {
    ensure!(
        &bytes[..4] == b"CRSS"
            && u16::from_le_bytes(bytes[4..6].try_into()?) == 1
            && u16::from_le_bytes(bytes[6..8].try_into()?) == 64,
        "Incompatible widget status"
    );
    let value = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    ensure!(
        value(52) == session
            && value(16) & !127 == 0
            && (48..=768).contains(&value(20))
            && value(32) <= 32768
            && value(36) <= 32768,
        "Invalid widget status"
    );
    let flags = value(16);
    ensure!(
        flags & 64 == 0 || flags & 32 != 0,
        "Presented widget pixels were not prepared"
    );
    Ok(Status {
        generation: u64::from_le_bytes(bytes[8..16].try_into()?),
        prepared: flags & 32 != 0,
        presented: flags & 64 != 0,
        widget: WidgetStatus {
            installed: true,
            window_open: false,
            connected: flags & 1 != 0,
            pinned: flags & 2 != 0,
            visible: flags & 4 != 0,
            click_through: flags & 8 != 0,
            positioned: flags & 16 != 0,
            dpi: value(20),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_recovery_requires_proof_the_last_host_exited() -> Result<()> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        assert!(LastHost::NeverConnected.gone());
        assert!(!LastHost::Observed(None).gone());
        let current = unsafe {
            use windows::Win32::System::Threading::{
                GetCurrentProcessId, OpenProcess, PROCESS_SYNCHRONIZE,
            };
            crosshair_platform::identity::owned(OpenProcess(
                PROCESS_SYNCHRONIZE,
                false,
                GetCurrentProcessId(),
            )?)
        };
        assert!(!LastHost::Observed(Some(current)).gone());
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--list")
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .spawn()?;
        let process = std::os::windows::io::AsHandle::as_handle(&child).try_clone_to_owned()?;
        child.wait()?;
        assert!(LastHost::Observed(Some(process)).gone());
        Ok(())
    }

    #[test]
    fn frame_preserves_asymmetric_origin_and_negative_monitor_position() {
        let frame = Frame {
            generation: 41,
            raster: Some(Arc::new(Raster {
                width: 5,
                height: 8,
                origin: [-3, 7],
                pixels: vec![0; 160],
            })),
            origin: [-1600, -40],
            center: true,
            close: false,
            prepare: false,
        };
        let bytes = header(&frame);
        assert_eq!(&bytes[..8], b"CRSF\x01\0\x28\0");
        assert_eq!(i32::from_le_bytes(bytes[28..32].try_into().unwrap()), -1598);
        assert_eq!(i32::from_le_bytes(bytes[32..36].try_into().unwrap()), -36);
        assert_eq!(bytes[36], 3);
        let mut frame = frame;
        frame.prepare = true;
        assert_eq!(header(&frame)[36], 11);
        frame.raster = None;
        frame.prepare = false;
        frame.center = false;
        frame.close = true;
        let closed = header(&frame);
        assert_eq!(closed[36], 4);
        assert_eq!(&closed[16..36], &[0; 20]);
    }
    #[test]
    fn status_rejects_wrong_session_flags_and_dpi() {
        let mut bytes = [0; 64];
        bytes[..8].copy_from_slice(b"CRSS\x01\0\x40\0");
        bytes[16..20].copy_from_slice(&31u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&144u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&8u32.to_le_bytes());
        bytes[8..16].copy_from_slice(&41u64.to_le_bytes());
        let status = decode_status(&bytes, 8).unwrap();
        assert!(status.widget.positioned);
        assert_eq!(status.generation, 41);
        assert!(decode_status(&bytes, 7).is_err());
        bytes[16] = 128;
        assert!(decode_status(&bytes, 8).is_err());
    }
}
