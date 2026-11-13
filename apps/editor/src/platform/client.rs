use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use crosshair_core::{AppState, Asset, Id, MAX_ASSET_BYTES, MAX_CONFIG_BYTES, Preset, protocol::*};
use crosshair_platform::{
    identity::{Identity, control_pipe_name},
    pipe::Pipe,
};
use crosshair_renderer::AssetData;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub scope: String,
    pub test_data: Option<PathBuf>,
    pub no_input: bool,
}

impl Options {
    pub fn parse() -> Result<Self> {
        let mut options = Self {
            scope: "app".into(),
            ..Default::default()
        };
        let mut arguments = std::env::args_os().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--ipc-scope") => {
                    options.scope = arguments
                        .next()
                        .context("--ipc-scope needs a value")?
                        .to_string_lossy()
                        .into()
                }
                Some("--test-data-dir") => {
                    options.test_data = Some(
                        arguments
                            .next()
                            .context("--test-data-dir needs a path")?
                            .into(),
                    )
                }
                Some("--no-input") => options.no_input = true,
                _ => bail!("Unknown editor argument"),
            }
        }
        ensure!(
            options.scope != "app" || options.test_data.is_none(),
            "Test storage requires an isolated IPC scope"
        );
        ensure!(
            !options.no_input || options.test_data.is_some(),
            "--no-input requires isolated test storage"
        );
        control_pipe_name(&options.scope)?;
        Ok(options)
    }
}

pub enum Event {
    Reused,
    Connected,
    State(AppState),
    Message(ServerMessage),
    Completed {
        tag: String,
        reply: Reply,
    },
    Failed {
        tag: String,
        message: String,
        conflict: bool,
    },
    Disconnected(String),
    Activity(Option<&'static str>),
}

type Events = Arc<dyn Fn(u64, Event) + Send + Sync>;

enum Job {
    Command(String, Command),
    Document(String, Preset, u64, bool),
    StageFile {
        path: PathBuf,
        purpose: StagingPurpose,
        tag: String,
    },
    ExportDraft {
        document: Preset,
        destination: PathBuf,
        format: DraftFormat,
    },
    Asset {
        id: Id,
        metadata: Asset,
        result: SyncSender<Result<AssetData>>,
    },
    StageAsset {
        data: AssetData,
        result: SyncSender<Result<Id>>,
    },
    ReadState {
        result: SyncSender<Result<AppState>>,
    },
    Refresh,
    Reconnect,
    Stop,
}

#[derive(Clone)]
pub struct Client {
    jobs: SyncSender<Job>,
    refresh_pending: Arc<AtomicBool>,
    pipe: Arc<Mutex<Option<Pipe>>>,
    source_assets: Arc<Mutex<BTreeMap<Id, AssetData>>>,
    pinned_assets: Arc<Mutex<BTreeSet<Id>>>,
    connection_epoch: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
}

impl Client {
    #[cfg(test)]
    pub fn disconnected_for_test() -> Self {
        let (jobs, _) = mpsc::sync_channel(1);
        Self {
            jobs,
            refresh_pending: Arc::new(AtomicBool::new(false)),
            pipe: Arc::new(Mutex::new(None)),
            source_assets: Arc::new(Mutex::new(BTreeMap::new())),
            pinned_assets: Arc::new(Mutex::new(BTreeSet::new())),
            connection_epoch: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn start(options: Options, events: Events) -> Self {
        let (jobs, receiver) = mpsc::sync_channel(64);
        let client = Self {
            jobs,
            refresh_pending: Arc::new(AtomicBool::new(false)),
            pipe: Arc::new(Mutex::new(None)),
            source_assets: Arc::new(Mutex::new(BTreeMap::new())),
            pinned_assets: Arc::new(Mutex::new(BTreeSet::new())),
            connection_epoch: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        let worker_client = client.clone();
        thread::Builder::new()
            .name("editor-ipc".into())
            .spawn(move || worker(options, receiver, worker_client, events))
            .expect("IPC worker must start");
        client
    }
    fn send(&self, job: Job) -> Result<()> {
        ensure!(
            !self.stopped.load(Ordering::Acquire),
            "The editor connection has closed"
        );
        self.jobs
            .try_send(job)
            .context("The engine is busy. Retry after the current operation.")
    }
    pub fn command(&self, tag: impl Into<String>, command: Command) -> Result<()> {
        self.send(Job::Command(tag.into(), command))
    }
    pub fn document(&self, tag: &str, preset: Preset, revision: u64) -> Result<()> {
        self.send(Job::Document(tag.into(), preset, revision, false))
    }
    pub fn preview(&self, preset: Preset) -> Result<()> {
        let revision = preset.revision;
        self.send(Job::Document("preview".into(), preset, revision, true))
    }
    pub fn stage_file(
        &self,
        path: PathBuf,
        purpose: StagingPurpose,
        tag: impl Into<String>,
    ) -> Result<()> {
        self.send(Job::StageFile {
            path,
            purpose,
            tag: tag.into(),
        })
    }
    pub fn export_draft(
        &self,
        document: Preset,
        destination: PathBuf,
        format: DraftFormat,
    ) -> Result<()> {
        self.send(Job::ExportDraft {
            document,
            destination,
            format,
        })
    }
    pub fn refresh(&self) {
        if !self.refresh_pending.swap(true, Ordering::AcqRel) && self.send(Job::Refresh).is_err() {
            self.refresh_pending.store(false, Ordering::Release);
        }
    }
    pub fn reconnect(&self) -> Result<()> {
        self.send(Job::Reconnect)
    }
    pub fn asset(&self, id: Id, metadata: Asset) -> Result<AssetData> {
        if let Some(data) = self.cached_asset(id) {
            return Ok(data);
        }
        let (result, receiver) = mpsc::sync_channel(1);
        self.send(Job::Asset {
            id,
            metadata,
            result,
        })?;
        receiver
            .recv_timeout(Duration::from_secs(660))
            .context("Image loading timed out")?
    }
    pub fn cached_asset(&self, id: Id) -> Option<AssetData> {
        self.source_assets.lock().ok()?.get(&id).cloned()
    }
    pub fn pin_assets(&self, ids: BTreeSet<Id>) {
        if let Ok(mut pinned) = self.pinned_assets.lock() {
            *pinned = ids;
        }
    }
    fn remember_asset(&self, id: Id, data: AssetData, pin: bool) {
        if pin && let Ok(mut pinned) = self.pinned_assets.lock() {
            pinned.insert(id);
        }
        if let (Ok(pinned), Ok(mut cache)) = (self.pinned_assets.lock(), self.source_assets.lock())
        {
            cache.insert(id, data);
            if cache.values().map(|data| data.bytes.len()).sum::<usize>() > 64 * 1024 * 1024 {
                cache.retain(|id, _| pinned.contains(id));
            }
        }
    }
    pub fn restage_asset(&self, data: AssetData) -> Result<Id> {
        let (result, receiver) = mpsc::sync_channel(1);
        self.send(Job::StageAsset { data, result })?;
        receiver
            .recv_timeout(Duration::from_secs(660))
            .context("Restoring this image timed out")?
    }
    pub fn state(&self) -> Result<AppState> {
        let (result, receiver) = mpsc::sync_channel(1);
        self.send(Job::ReadState { result })?;
        receiver
            .recv_timeout(Duration::from_secs(660))
            .context("Reading engine state timed out")?
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.connection_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.jobs.try_send(Job::Stop);
        if let Ok(pipe) = self.pipe.lock()
            && let Some(pipe) = pipe.as_ref()
        {
            pipe.cancel();
        }
    }
    pub fn accepts(&self, epoch: u64) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self.connection_epoch.load(Ordering::Acquire) == epoch
    }
}

struct Connection {
    pipe: Pipe,
    responses: Receiver<Result<Response>>,
    next_id: u64,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.pipe.cancel();
    }
}

impl Connection {
    fn open(
        options: &Options,
        events: Arc<dyn Fn(Event) + Send + Sync>,
        launch: bool,
        stopped: &AtomicBool,
    ) -> Result<Self> {
        let identity = Identity::current()?;
        let name = control_pipe_name(&options.scope)?;
        let mut pipe = Pipe::connect(&name, &identity, &["crosshair-engine.exe"]);
        if pipe.is_err() && launch {
            ensure!(
                !stopped.load(Ordering::Acquire),
                "The editor connection has closed"
            );
            launch_engine(options)?;
            for delay in [100, 200, 400, 800, 1200, 1600] {
                thread::sleep(Duration::from_millis(delay));
                ensure!(
                    !stopped.load(Ordering::Acquire),
                    "The editor connection has closed"
                );
                pipe = Pipe::connect(&name, &identity, &["crosshair-engine.exe"]);
                if pipe.is_ok() {
                    break;
                }
            }
        }
        let pipe = pipe.context(
            "Could not connect to crosshair-rs. Retry, or restart the engine from the tray.",
        )?;
        let reader = pipe.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let reader_cancelled = cancelled.clone();
        let (sender, responses) = mpsc::sync_channel(8);
        thread::Builder::new()
            .name("editor-events".into())
            .spawn(move || {
                loop {
                    match reader.read_message::<ServerMessage>() {
                        Ok(ServerMessage::Response(response)) => {
                            if sender.send(Ok(response)).is_err() {
                                break;
                            }
                        }
                        Ok(message) => {
                            let closing = matches!(message, ServerMessage::Closing);
                            events(Event::Message(message));
                            if closing {
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!("Engine connection ended: {error:#}");
                            let _ = sender.send(Err(error));
                            if !reader_cancelled.load(Ordering::Acquire) {
                                events(Event::Disconnected(
                                    "The engine disconnected. Reconnect to continue.".into(),
                                ));
                            }
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            pipe,
            responses,
            next_id: 1,
            cancelled,
        })
    }
    fn request(&mut self, command: Command) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;
        let deadline = response_deadline(&command);
        self.pipe.write_message(&Request::new(id, command))?;
        let response = self
            .responses
            .recv_timeout(deadline)
            .context("The engine operation exceeded its response deadline")??;
        ensure!(response.id == id, "The engine sent an unexpected response");
        Ok(response)
    }
    fn reply(&mut self, command: Command) -> Result<Reply> {
        self.request(command)?
            .result
            .map_err(|error| anyhow::anyhow!(error.message))
    }
    fn snapshot(&mut self) -> Result<AppState> {
        let Reply::Snapshot { token, bytes, .. } = self.reply(Command::Snapshot)? else {
            bail!("The engine sent an invalid snapshot");
        };
        ensure!(
            bytes <= MAX_CONFIG_BYTES as u64,
            "The engine snapshot exceeds the size limit"
        );
        let mut data = Vec::with_capacity(bytes as usize);
        loop {
            let Reply::Chunk {
                bytes: chunk,
                complete,
            } = self.reply(Command::ReadSnapshot {
                token,
                offset: data.len() as u64,
            })?
            else {
                bail!("The engine sent an invalid snapshot chunk");
            };
            ensure!(
                chunk.len() <= CHUNK_BYTES && data.len() + chunk.len() <= bytes as usize,
                "The snapshot chunk exceeds its size limit"
            );
            ensure!(
                !chunk.is_empty() || complete,
                "The snapshot stopped before completion"
            );
            data.extend(chunk);
            if complete {
                break;
            }
        }
        ensure!(
            data.len() == bytes as usize,
            "The engine snapshot is incomplete"
        );
        let state: AppState = serde_json::from_slice(&data)?;
        crosshair_core::validation::validate_state(&state)?;
        Ok(state)
    }
    fn document(
        &mut self,
        preset: Preset,
        expected_revision: u64,
        preview: bool,
    ) -> Result<Response> {
        let bytes = serde_json::to_vec(&preset)?;
        if bytes.len() < MAX_FRAME_BYTES / 2 {
            return self.request(if preview {
                Command::PreviewPreset {
                    preset,
                    expected_revision,
                }
            } else {
                Command::ReplacePreset {
                    preset,
                    expected_revision,
                }
            });
        }
        self.stage_bytes(
            &bytes,
            StagingPurpose::Document {
                preset: preset.id,
                expected_revision,
                preview,
            },
        )
    }
    fn stage(&mut self, path: PathBuf, purpose: StagingPurpose) -> Result<Response> {
        let mut source = std::fs::File::open(path)?;
        let length = source.metadata()?.len();
        let Reply::Staging {
            token,
            path,
            max_bytes,
        } = self.reply(Command::BeginStaging { purpose })?
        else {
            bail!("The engine could not stage this file");
        };
        let result = (|| {
            ensure!(length <= max_bytes, "This file exceeds the import limit");
            write_staged(Path::new(&path), &mut source, max_bytes)?;
            self.request(Command::CommitStaging { token })
        })();
        if result.is_err() {
            let _ = self.reply(Command::DiscardStaging { token });
        }
        result
    }
    fn export_draft(
        &mut self,
        document: Preset,
        destination: PathBuf,
        format: DraftFormat,
    ) -> Result<Response> {
        let bytes = serde_json::to_vec(&document)?;
        let purpose = StagingPurpose::ExportDraft {
            destination: destination.to_string_lossy().into(),
            format,
        };
        self.stage_bytes(&bytes, purpose)
    }
    fn stage_bytes(&mut self, bytes: &[u8], purpose: StagingPurpose) -> Result<Response> {
        let Reply::Staging {
            token,
            path,
            max_bytes,
        } = self.reply(Command::BeginStaging { purpose })?
        else {
            bail!("The engine could not stage this transfer");
        };
        let result = (|| {
            ensure!(
                bytes.len() as u64 <= max_bytes,
                "The draft exceeds the document limit"
            );
            write_staged(Path::new(&path), bytes, max_bytes)?;
            self.request(Command::CommitStaging { token })
        })();
        if result.is_err() {
            let _ = self.reply(Command::DiscardStaging { token });
        }
        result
    }
    fn asset(&mut self, id: Id, metadata: Asset) -> Result<AssetData> {
        ensure!(
            metadata.source_bytes <= MAX_ASSET_BYTES,
            "Image exceeds the asset limit"
        );
        let mut data = Vec::new();
        loop {
            let Reply::Chunk { bytes, complete } = self.reply(Command::ReadAsset {
                asset: id,
                offset: data.len() as u64,
            })?
            else {
                bail!("Invalid image response");
            };
            ensure!(
                bytes.len() <= CHUNK_BYTES && (data.len() + bytes.len()) as u64 <= MAX_ASSET_BYTES,
                "Image exceeds the asset limit"
            );
            ensure!(!bytes.is_empty() || complete, "Image data is incomplete");
            data.extend(bytes);
            if complete {
                break;
            }
        }
        ensure!(
            data.len() as u64 == metadata.source_bytes,
            "Image data is incomplete"
        );
        Ok(AssetData {
            kind: metadata.kind,
            bytes: data.into(),
        })
    }
}

fn launch_engine(options: &Options) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let engine = std::env::current_exe()?.with_file_name("crosshair-engine.exe");
    ensure!(
        engine.is_file(),
        "crosshair-engine.exe is missing. Reinstall the application."
    );
    let mut process = std::process::Command::new(engine);
    process.creation_flags(0x0800_0000);
    if options.scope != "app" {
        process.arg("--ipc-scope").arg(&options.scope);
    }
    if let Some(path) = &options.test_data {
        process.arg("--test-data-dir").arg(path);
    }
    if options.no_input {
        process.arg("--no-input");
    }
    process
        .spawn()
        .context("Could not start the crosshair engine")?;
    Ok(())
}

fn worker(options: Options, jobs: Receiver<Job>, client: Client, events: Events) {
    let mut connection = connect(&options, &client, &events, true);
    while let Ok(job) = jobs.recv() {
        if client.stopped.load(Ordering::Acquire) || matches!(job, Job::Stop) {
            break;
        }
        if matches!(job, Job::Reconnect) {
            drop(connection.take());
            if let Ok(mut pipe) = client.pipe.lock() {
                *pipe = None;
            }
            connection = connect(&options, &client, &events, true);
            continue;
        }
        let epoch = client.connection_epoch.load(Ordering::Acquire);
        let notify = |event| events(epoch, event);
        let Some(current) = &mut connection else {
            match job {
                Job::Asset { result, .. } => {
                    let _ = result.send(Err(anyhow::anyhow!("The engine is disconnected")));
                }
                Job::StageAsset { result, .. } => {
                    let _ = result.send(Err(anyhow::anyhow!("The engine is disconnected")));
                }
                Job::ReadState { result } => {
                    let _ = result.send(Err(anyhow::anyhow!("The engine is disconnected")));
                }
                _ => notify(Event::Failed {
                    tag: "connection".into(),
                    message: "The engine is disconnected. Reconnect to continue.".into(),
                    conflict: false,
                }),
            }
            client.refresh_pending.store(false, Ordering::Release);
            continue;
        };
        let activity = match &job {
            Job::StageFile { .. } => Some("Importing…"),
            Job::ExportDraft { .. } | Job::Command(_, Command::Export { .. }) => Some("Exporting…"),
            Job::Command(_, Command::RecoverBackup) => Some("Restoring backup…"),
            Job::Command(_, Command::ResetSettings) => Some("Resetting settings…"),
            _ => None,
        };
        if activity.is_some() {
            notify(Event::Activity(activity));
        }
        let (tag, result) = match job {
            Job::Command(tag, command) => (tag, current.request(command)),
            Job::Document(tag, preset, revision, preview) => {
                (tag, current.document(preset, revision, preview))
            }
            Job::StageFile { path, purpose, tag } => {
                let result = current.stage(path, purpose);
                if let Ok(Response {
                    result: Ok(Reply::Asset { id, .. }),
                    ..
                }) = &result
                    && let Ok(state) = current.snapshot()
                {
                    if let Some(metadata) = state.assets.get(id)
                        && let Ok(data) = current.asset(*id, metadata.clone())
                    {
                        client.remember_asset(*id, data, true);
                    }
                    notify(Event::State(state));
                }
                (tag, result)
            }
            Job::ExportDraft {
                document,
                destination,
                format,
            } => (
                "export".into(),
                current.export_draft(document, destination, format),
            ),
            Job::Refresh => {
                // Allow one follow-up snapshot when a change arrives during this transfer.
                client.refresh_pending.store(false, Ordering::Release);
                let result = current.snapshot();
                match result {
                    Ok(state) => notify(Event::State(state)),
                    Err(error) => notify(Event::Failed {
                        tag: "refresh".into(),
                        message: error.to_string(),
                        conflict: false,
                    }),
                }
                continue;
            }
            Job::Asset {
                id,
                metadata,
                result,
            } => {
                let asset = current.asset(id, metadata);
                if let Ok(data) = &asset {
                    client.remember_asset(id, data.clone(), false);
                }
                let _ = result.send(asset);
                continue;
            }
            Job::StageAsset { data, result } => {
                let outcome = current
                    .stage_bytes(&data.bytes, StagingPurpose::Image)
                    .and_then(|response| match response.result {
                        Ok(Reply::Asset { id, .. }) => {
                            client.remember_asset(id, data, true);
                            Ok(id)
                        }
                        Ok(_) => bail!("The engine sent an invalid image result"),
                        Err(error) => bail!(error.message),
                    });
                let _ = result.send(outcome);
                continue;
            }
            Job::ReadState { result } => {
                let _ = result.send(current.snapshot());
                continue;
            }
            Job::Reconnect | Job::Stop => unreachable!(),
        };
        if activity.is_some() {
            notify(Event::Activity(None));
        }
        match result {
            Ok(Response {
                result: Ok(reply), ..
            }) => notify(Event::Completed { tag, reply }),
            Ok(Response {
                result: Err(error), ..
            }) => notify(Event::Failed {
                tag,
                message: error.message,
                conflict: matches!(error.code, ErrorCode::Conflict),
            }),
            Err(error) => {
                let message = error.to_string();
                notify(Event::Failed {
                    tag,
                    message: message.clone(),
                    conflict: false,
                });
                connection = None;
                if let Ok(mut pipe) = client.pipe.lock() {
                    *pipe = None;
                }
                notify(Event::Disconnected(message));
            }
        }
    }
}

fn response_deadline(command: &Command) -> Duration {
    if matches!(
        command,
        Command::CommitStaging { .. }
            | Command::Export { .. }
            | Command::RecoverBackup
            | Command::ResetSettings
    ) {
        Duration::from_secs(600)
    } else {
        Duration::from_secs(35)
    }
}

fn connect(
    options: &Options,
    client: &Client,
    events: &Events,
    launch: bool,
) -> Option<Connection> {
    if client.stopped.load(Ordering::Acquire) {
        return None;
    }
    let current_epoch = client.connection_epoch.fetch_add(1, Ordering::AcqRel) + 1;
    let epoch = client.connection_epoch.clone();
    let destination = events.clone();
    let events: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
        if epoch.load(Ordering::Acquire) == current_epoch {
            destination(current_epoch, event);
        }
    });
    let result = (|| -> Result<Connection> {
        let mut connection = Connection::open(options, events.clone(), launch, &client.stopped)?;
        if let Ok(mut pipe) = client.pipe.lock() {
            *pipe = Some(connection.pipe.clone());
        }
        let response = connection.request(Command::Subscribe)?;
        let reply = match response.result {
            Ok(reply) => reply,
            Err(error) if matches!(error.code, ErrorCode::Busy) => {
                events(Event::Reused);
                bail!("The existing editor was focused");
            }
            Err(error) => bail!(error.message),
        };
        let state = connection.snapshot()?;
        events(Event::Connected);
        if let Reply::Status { status } = reply {
            events(Event::Message(ServerMessage::Changed {
                revision: 0,
                document: None,
                structure: true,
                status,
            }));
        }
        events(Event::State(state));
        Ok(connection)
    })();
    match result {
        Ok(connection) => Some(connection),
        Err(error) => {
            events(Event::Disconnected(error.to_string()));
            None
        }
    }
}

// Return only after closing the writer; CommitStaging opens the file exclusively.
fn write_staged(path: &Path, source: impl Read, max_bytes: u64) -> Result<()> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    let copied = std::io::copy(&mut source.take(max_bytes.saturating_add(1)), &mut file)?;
    ensure!(copied <= max_bytes, "This transfer exceeds its size limit");
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_writer_closes_before_commit_even_when_input_exceeds_limit() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transfer");
        std::fs::write(&path, []).unwrap();
        write_staged(&path, &b"abc"[..], 3).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        drop(
            OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap(),
        );
        assert!(write_staged(&path, &b"abcd"[..], 3).is_err());
        drop(
            OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap(),
        );
    }

    #[test]
    fn closing_invalidates_queued_events_and_rejects_reconnects() {
        let (jobs, receiver) = mpsc::sync_channel(64);
        let client = Client {
            jobs,
            refresh_pending: Arc::new(AtomicBool::new(false)),
            pipe: Arc::new(Mutex::new(None)),
            source_assets: Arc::new(Mutex::new(BTreeMap::new())),
            pinned_assets: Arc::new(Mutex::new(BTreeSet::new())),
            connection_epoch: Arc::new(AtomicU64::new(1)),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        assert!(client.accepts(1));
        client.connection_epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!client.accepts(1));
        assert!(client.accepts(2));
        for _ in 0..100 {
            client.refresh();
        }
        assert!(matches!(receiver.try_recv(), Ok(Job::Refresh)));
        assert!(receiver.try_recv().is_err());
        client.reconnect().unwrap();
        client.stop();
        assert!(!client.accepts(2));
        assert!(!client.accepts(3));
        assert!(client.reconnect().is_err());
        assert!(client.command("late", Command::ToggleVisibility).is_err());
        assert!(client.stopped.load(Ordering::Acquire));
    }

    #[test]
    fn portable_io_has_time_to_finish_without_weakening_control_deadlines() {
        assert_eq!(
            response_deadline(&Command::CommitStaging { token: Id::nil() }),
            Duration::from_secs(600)
        );
        assert_eq!(
            response_deadline(&Command::Export {
                kind: ExportKind::All,
                destination: "export.json".into()
            }),
            Duration::from_secs(600)
        );
        assert_eq!(
            response_deadline(&Command::Snapshot),
            Duration::from_secs(35)
        );
        assert_eq!(
            response_deadline(&Command::CancelCapture { token: Id::nil() }),
            Duration::from_secs(35)
        );
    }

    #[test]
    fn pinned_draft_images_survive_eviction_and_disconnection() {
        let (jobs, _receiver) = mpsc::sync_channel(64);
        let client = Client {
            jobs,
            refresh_pending: Arc::new(AtomicBool::new(false)),
            pipe: Arc::new(Mutex::new(None)),
            source_assets: Arc::new(Mutex::new(BTreeMap::new())),
            pinned_assets: Arc::new(Mutex::new(BTreeSet::new())),
            connection_epoch: Arc::new(AtomicU64::new(1)),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        let id = Id::new_v4();
        let image = AssetData {
            kind: crosshair_core::AssetKind::Png,
            bytes: vec![0; 8 * 1024 * 1024].into(),
        };
        client.remember_asset(id, image.clone(), true);
        for _ in 0..8 {
            client.remember_asset(Id::new_v4(), image.clone(), false);
        }
        assert_eq!(client.source_assets.lock().unwrap().len(), 1);
        client.stop();
        let cached = client
            .cached_asset(id)
            .expect("offline undo keeps its source");
        assert!(Arc::ptr_eq(&cached.bytes, &image.bytes));
    }
}
