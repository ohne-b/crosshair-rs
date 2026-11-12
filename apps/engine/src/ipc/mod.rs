use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, anyhow};
use crosshair_core::{
    Id,
    protocol::{Request, ServerMessage},
};
use crosshair_platform::{
    identity::Identity,
    pipe::{Listener, Pipe, StopSignal},
};

#[derive(Clone)]
pub struct Client {
    pub id: Id,
    sender: SyncSender<Outgoing>,
    pipe: Pipe,
}

impl Client {
    pub fn send(&self, message: ServerMessage) {
        // A stalled editor cannot block the engine or grow an unbounded event queue.
        if self
            .sender
            .try_send(Outgoing::Message(Box::new(message)))
            .is_err()
        {
            self.pipe.cancel();
        }
    }

    pub fn flush(&self) {
        let (sender, receiver) = mpsc::sync_channel(1);
        if self.sender.try_send(Outgoing::Flush(sender)).is_ok() {
            let _ = receiver.recv_timeout(Duration::from_secs(2));
        }
    }
}

enum Outgoing {
    Message(Box<ServerMessage>),
    Flush(SyncSender<()>),
    Close,
}

pub enum Event {
    Connected(Client),
    Request(Id, Request),
    Disconnected(Id),
}

pub struct Server {
    stop: StopSignal,
    stopping: Arc<AtomicBool>,
    clients: Arc<Mutex<BTreeMap<Id, Pipe>>>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(
        name: String,
        identity: Identity,
        notify: impl Fn(Event) -> bool + Send + Sync + 'static,
    ) -> Result<Self> {
        let (started, ready) = mpsc::sync_channel(1);
        let stopping = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(Mutex::new(BTreeMap::<Id, Pipe>::new()));
        let notify = Arc::new(notify);
        let worker = {
            let stopping = stopping.clone();
            let clients = clients.clone();
            thread::Builder::new()
                .name("control listener".into())
                .spawn(move || {
                    let mut listener = match Listener::bind(&name, identity, false) {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = started.send(Err(error));
                            return;
                        }
                    };
                    if started.send(Ok(listener.stop_signal())).is_err() {
                        return;
                    }
                    let mut workers: Vec<JoinHandle<()>> = Vec::new();
                    while !stopping.load(Ordering::Acquire) {
                        let pipe = match listener.accept() {
                            Ok(pipe) => pipe,
                            Err(_) => continue,
                        };
                        for index in (0..workers.len()).rev() {
                            if workers[index].is_finished() {
                                let _ = workers.swap_remove(index).join();
                            }
                        }
                        if clients.lock().unwrap().len() >= 2 {
                            pipe.cancel();
                            continue;
                        }
                        let id = Id::new_v4();
                        clients.lock().unwrap().insert(id, pipe.clone());
                        let client_registry = clients.clone();
                        let notify = notify.clone();
                        let failure_pipe = pipe.clone();
                        let worker =
                            thread::Builder::new()
                                .name("control client".into())
                                .spawn(move || {
                                    let (sender, outgoing) = mpsc::sync_channel(16);
                                    let client = Client {
                                        id,
                                        sender: sender.clone(),
                                        pipe: pipe.clone(),
                                    };
                                    let writer_pipe = pipe.clone();
                                    let writer = thread::Builder::new()
                                        .name("control events".into())
                                        .spawn(move || {
                                            for message in outgoing {
                                                match message {
                                                    Outgoing::Message(message) => {
                                                        if writer_pipe
                                                            .write_message(&message)
                                                            .is_err()
                                                        {
                                                            break;
                                                        }
                                                    }
                                                    Outgoing::Flush(sender) => {
                                                        let _ = sender.send(());
                                                    }
                                                    Outgoing::Close => break,
                                                }
                                            }
                                            writer_pipe.cancel();
                                        });
                                    if writer.is_ok() && notify(Event::Connected(client)) {
                                        while let Ok(request) = pipe.read_message::<Request>() {
                                            if !notify(Event::Request(id, request)) {
                                                break;
                                            }
                                        }
                                    }
                                    pipe.cancel();
                                    client_registry.lock().unwrap().remove(&id);
                                    let _ = sender.try_send(Outgoing::Close);
                                    notify(Event::Disconnected(id));
                                    // The owner drops the Client sender on disconnect or shutdown.
                                    if let Ok(writer) = writer {
                                        let _ = writer.join();
                                    }
                                });
                        match worker {
                            Ok(worker) => workers.push(worker),
                            Err(_) => {
                                failure_pipe.cancel();
                                self_remove(&clients, id);
                            }
                        }
                    }
                    for pipe in clients.lock().unwrap().values() {
                        pipe.cancel();
                    }
                    for worker in workers {
                        let _ = worker.join();
                    }
                })?
        };
        let stop = ready
            .recv()
            .map_err(|_| anyhow!("The control listener stopped during startup"))??;
        Ok(Self {
            stop,
            stopping,
            clients,
            worker: Some(worker),
        })
    }

    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.stop.signal();
        for pipe in self.clients.lock().unwrap().values() {
            pipe.cancel();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn self_remove(clients: &Mutex<BTreeMap<Id, Pipe>>, id: Id) {
    clients.lock().unwrap().remove(&id);
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
