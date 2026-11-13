use std::{
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Result, bail, ensure};
use crosshair_core::{DiscordStyle, Settings};
use crosshair_platform::{
    identity::Identity,
    pipe::{Pipe, StopSignal},
};
use serde_json::{Value, json};

const APPLICATION_ID: &str = "1570691376738209834";
const ASSET_KEY: &str = "crosshair_rs";
const IO_TIMEOUT: Duration = Duration::from_millis(500);
const DEBOUNCE: Duration = Duration::from_millis(350);
const UPDATE_INTERVAL: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    Simple(bool),
    Quote,
}

impl Activity {
    fn payload(self) -> Value {
        let (details, state) = match self {
            Self::Simple(visible) => (
                if visible {
                    "Overlay active"
                } else {
                    "Overlay hidden"
                },
                None,
            ),
            Self::Quote => ("I've got you in my sights.", Some("— Soldier: 76")),
        };
        let mut activity = json!({
            "details": details,
            "assets": { "large_image": ASSET_KEY, "large_text": "Crosshair-RS" },
            "buttons": [
                { "label": "Download", "url": "https://github.com/ohne-b/crosshair-rs/releases" }
            ]
        });
        if let Some(state) = state {
            activity["state"] = state.into();
        }
        activity
    }
}

struct State {
    activity: Option<Activity>,
    changed: Instant,
    stop: bool,
}

struct Worker {
    state: Arc<Mutex<State>>,
    wake: StopSignal,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub struct Presence {
    worker: Option<Worker>,
}

impl Presence {
    pub fn update(&mut self, settings: &Settings, visible: bool) {
        let activity = settings
            .discord_presence
            .then_some(match settings.discord_style {
                DiscordStyle::Simple => Activity::Simple(visible),
                DiscordStyle::Quote => Activity::Quote,
            });
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished())
        {
            self.worker = None;
        }
        if self.worker.is_none() && activity.is_some() {
            self.worker = Worker::start().ok();
        }
        if let Some(worker) = &self.worker {
            let mut state = worker.state.lock().unwrap();
            if state.activity != activity {
                state.activity = activity;
                state.changed = Instant::now();
                worker.wake.signal();
            }
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.state.lock().unwrap().stop = true;
            worker.wake.signal();
            let _ = worker.thread.join();
        }
    }
}

impl Worker {
    fn start() -> Result<Self> {
        let state = Arc::new(Mutex::new(State {
            activity: None,
            changed: Instant::now(),
            stop: false,
        }));
        let wake = StopSignal::new()?;
        let shared = state.clone();
        let notification = wake.clone();
        let thread = thread::Builder::new()
            .name("discord-presence".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                if let Ok(identity) = Identity::current() {
                    run(shared, notification, || Session::connect(&identity));
                }
            })?;
        Ok(Self {
            state,
            wake,
            thread,
        })
    }
}

fn run(shared: Arc<Mutex<State>>, wake: StopSignal, mut connect: impl FnMut() -> Result<Session>) {
    let mut session: Option<Session> = None;
    let mut sent = None;
    let mut next_send = Instant::now();
    let mut retry = Instant::now();
    let mut backoff = Duration::from_secs(1);
    loop {
        let (activity, changed, stop) = {
            let state = shared.lock().unwrap();
            wake.reset();
            (state.activity, state.changed, state.stop)
        };
        if activity.is_none() || stop {
            if let Some(mut connection) = session.take() {
                // Closing the pipe also clears presence if Discord cannot acknowledge cleanup.
                let _ = connection.send_activity(None, Instant::now() + Duration::from_millis(150));
            }
            sent = None;
            retry = Instant::now();
            backoff = Duration::from_secs(1);
            if stop {
                return;
            }
            wake.wait(None);
            continue;
        }
        if session.is_none() {
            if Instant::now() < retry {
                wake.wait(Some(retry.saturating_duration_since(Instant::now())));
                continue;
            }
            match connect() {
                Ok(connection) => {
                    session = Some(connection);
                    sent = None;
                    next_send = Instant::now();
                }
                Err(_) => {
                    retry = Instant::now() + backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
            continue;
        }
        let connection = session.as_mut().unwrap();
        let due = (sent != activity).then_some((changed + DEBOUNCE).max(next_send));
        let result = if due.is_some_and(|due| Instant::now() >= due) {
            connection.set_activity(activity).map(|()| {
                sent = activity;
                next_send = Instant::now() + UPDATE_INTERVAL;
                backoff = Duration::from_secs(1);
            })
        } else {
            connection.wait(&wake, due)
        };
        if result.is_err() {
            session = None;
            retry = Instant::now() + backoff;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }
}

struct Session {
    pipe: Pipe,
    nonce: u64,
}

impl Session {
    fn connect(identity: &Identity) -> Result<Self> {
        let pipe = (0..10)
            .find_map(|index| Pipe::connect_discord(index, identity).ok())
            .ok_or_else(|| anyhow::anyhow!("Discord is unavailable"))?;
        Self::handshake(pipe)
    }

    fn handshake(pipe: Pipe) -> Result<Self> {
        let mut session = Self { pipe, nonce: 0 };
        let deadline = Instant::now() + IO_TIMEOUT;
        session.send(
            0,
            &serde_json::to_vec(&json!({"v": 1, "client_id": APPLICATION_ID}))?,
            deadline,
        )?;
        let ready = session.receive(deadline)?;
        ensure!(
            ready["cmd"] == "DISPATCH" && ready["evt"] == "READY",
            "Discord handshake was rejected"
        );
        Ok(session)
    }

    fn send(&self, opcode: u32, bytes: &[u8], deadline: Instant) -> Result<()> {
        ensure!(Instant::now() < deadline, "Discord request timed out");
        ensure!(bytes.len() <= MAX_FRAME, "Discord frame is too large");
        let mut frame = Vec::with_capacity(8 + bytes.len());
        frame.extend_from_slice(&opcode.to_le_bytes());
        frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        frame.extend_from_slice(bytes);
        self.pipe.write_all(&frame, deadline)
    }

    fn send_activity(&mut self, activity: Option<Activity>, deadline: Instant) -> Result<String> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let request = json!({"cmd": "SET_ACTIVITY", "nonce": nonce,
            "args": {"pid": std::process::id(), "activity": activity.map(Activity::payload)}});
        self.send(1, &serde_json::to_vec(&request)?, deadline)?;
        Ok(nonce)
    }

    fn set_activity(&mut self, activity: Option<Activity>) -> Result<()> {
        let deadline = Instant::now() + IO_TIMEOUT;
        let nonce = self.send_activity(activity, deadline)?;
        loop {
            let reply = self.receive(deadline)?;
            ensure!(reply["evt"] != "ERROR", "Discord rejected activity");
            if reply["nonce"] == nonce {
                ensure!(
                    reply["cmd"] == "SET_ACTIVITY",
                    "Unexpected Discord response"
                );
                return Ok(());
            }
        }
    }

    fn receive(&mut self, deadline: Instant) -> Result<Value> {
        loop {
            ensure!(Instant::now() < deadline, "Discord request timed out");
            let mut header = [0; 8];
            self.pipe.read_exact(&mut header, deadline)?;
            if let Some(value) = self.frame(header, deadline)? {
                return Ok(value);
            }
        }
    }

    fn frame(&self, header: [u8; 8], deadline: Instant) -> Result<Option<Value>> {
        let opcode = u32::from_le_bytes(header[..4].try_into()?);
        let length = u32::from_le_bytes(header[4..].try_into()?) as usize;
        ensure!(length <= MAX_FRAME, "Discord frame is too large");
        let mut bytes = vec![0; length];
        self.pipe.read_exact(&mut bytes, deadline)?;
        match opcode {
            1 => Ok(Some(serde_json::from_slice(&bytes)?)),
            3 => {
                self.send(4, &bytes, deadline)?;
                Ok(None)
            }
            4 => Ok(None),
            _ => bail!("Discord connection closed or sent an invalid opcode"),
        }
    }

    fn wait(&mut self, wake: &StopSignal, deadline: Option<Instant>) -> Result<()> {
        let mut header = [0; 8];
        if let Some(count) = self.pipe.read_or_wake(&mut header, deadline, wake)? {
            ensure!(count > 0, "Discord disconnected");
            let deadline = Instant::now() + IO_TIMEOUT;
            self.pipe.read_exact(&mut header[count..], deadline)?;
            if let Some(reply) = self.frame(header, deadline)? {
                ensure!(reply["evt"] != "ERROR", "Discord reported an error");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
