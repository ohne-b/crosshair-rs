mod discovery;
mod helper;
mod http;
mod manifest;
mod package;
#[cfg(test)]
mod tests;

use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use crosshair_core::{
    Id,
    protocol::{PRODUCT_VERSION, UpdatePhase, UpdateStatus},
};

pub use helper::{
    RestartIntent, VerifiedHandoff, acknowledge_recovery, launch_handoff, recover, run_helper,
};

#[derive(Clone)]
pub struct Updates(Arc<Inner>);
struct Inner {
    root: PathBuf,
    state: Mutex<State>,
    notify: Box<dyn Fn(UpdateStatus) + Send + Sync>,
}
struct State {
    status: UpdateStatus,
    available: Option<manifest::Manifest>,
    ready: Option<Ready>,
    worker: Option<Arc<http::Cancellation>>,
}
struct Ready {
    job: Id,
    manifest: manifest::Manifest,
    file: File,
    bytes: u64,
    sha256: String,
    family: String,
}

pub fn build_configuration() -> serde_json::Value {
    serde_json::json!({
        "version": PRODUCT_VERSION,
        "packageName": manifest::PACKAGE_NAME,
        "updatePublicKey": option_env!("CROSSHAIR_UPDATE_PUBLIC_KEY"),
        "packagePublisher": option_env!("CROSSHAIR_PACKAGE_PUBLISHER"),
        "updatesConfigured": manifest::public_key().is_ok() && manifest::publisher().is_ok(),
    })
}

impl Updates {
    pub fn new(
        root: PathBuf,
        notify: impl Fn(UpdateStatus) + Send + Sync + 'static,
    ) -> Result<Self> {
        let root = root.join("updates");
        helper::private_directory(&root)?;
        let root = root.canonicalize()?;
        helper::clean_orphans(&root)?;
        Ok(Self(Arc::new(Inner {
            root,
            state: Mutex::new(State {
                status: UpdateStatus::default(),
                available: None,
                ready: None,
                worker: None,
            }),
            notify: Box::new(notify),
        })))
    }

    pub fn status(&self) -> UpdateStatus {
        self.0.state.lock().unwrap().status.clone()
    }
    pub fn report_recovery(&self, recovery: &helper::Recovery) {
        self.change(|state| {
            state.status.phase = if recovery.succeeded {
                UpdatePhase::Current
            } else {
                UpdatePhase::Failed
            };
            state.status.current_version = recovery
                .installed_version
                .clone()
                .unwrap_or_else(|| PRODUCT_VERSION.into());
            // Recovery has no authenticated download manifest; check again before offering Download.
            state.status.version = None;
            state.status.error = recovery.error.clone();
        });
    }
    pub fn check(&self, allow_prereleases: bool) -> Result<()> {
        self.start(false, allow_prereleases)
    }
    pub fn download(&self) -> Result<()> {
        self.start(true, false)
    }
    pub fn ensure_idle(&self) -> Result<()> {
        let state = self.0.state.lock().unwrap();
        ensure!(
            state.worker.is_none() && state.status.phase != UpdatePhase::Installing,
            "Finish or cancel the update before changing prerelease settings"
        );
        Ok(())
    }
    pub fn reset_offer(&self) {
        self.change(|state| {
            state.available = None;
            state.status.phase = UpdatePhase::Idle;
            state.status.version = None;
            state.status.error = None;
            state.status.downloaded = 0;
            state.status.total = None;
            // The next check or startup cleans the staged files; Idle prevents installation.
        });
    }
    pub fn cancel(&self) {
        if let Some(worker) = &self.0.state.lock().unwrap().worker {
            worker.cancel();
        }
    }

    pub fn installation_failed(&self, message: String) {
        self.change(|state| {
            state.status.phase = if state.ready.as_ref().is_some_and(|ready| {
                state
                    .available
                    .as_ref()
                    .is_some_and(|available| available.version == ready.manifest.version)
                    && state.status.version.as_deref() == Some(ready.manifest.version.as_str())
            }) {
                UpdatePhase::Ready
            } else {
                UpdatePhase::Failed
            };
            state.status.error = Some(message);
        });
    }

    pub fn prepare_install(&self, restart: RestartIntent) -> Result<VerifiedHandoff> {
        let (ready, status) = {
            let mut state = self.0.state.lock().unwrap();
            ensure!(
                state.worker.is_none() && state.status.phase == UpdatePhase::Ready,
                "The update is not ready to install"
            );
            let ready = state
                .ready
                .as_ref()
                .context("The verified update is unavailable")?;
            let ready = Ready {
                job: ready.job,
                manifest: ready.manifest.clone(),
                file: ready.file.try_clone()?,
                bytes: ready.bytes,
                sha256: ready.sha256.clone(),
                family: ready.family.clone(),
            };
            state.status.phase = UpdatePhase::Installing;
            state.status.error = None;
            state.status.revision = state.status.revision.saturating_add(1);
            (ready, state.status.clone())
        };
        (self.0.notify)(status);
        // Verification can be slow. Keep the status mutex
        // available to the engine's message thread throughout preparation.
        match helper::prepare(&self.0.root, &ready, restart) {
            Ok(handoff) => Ok(handoff),
            Err(error) => {
                self.installation_failed(format!("{error:#}"));
                Err(error)
            }
        }
    }

    fn start(&self, download: bool, allow_prereleases: bool) -> Result<()> {
        let cancellation = Arc::new(http::Cancellation::default());
        {
            let mut state = self.0.state.lock().unwrap();
            ensure!(
                state.worker.is_none() && state.status.phase != UpdatePhase::Installing,
                "An update operation is already running"
            );
            ensure!(
                !download || state.available.is_some(),
                "Check for updates before downloading"
            );
            state.worker = Some(cancellation.clone());
        }
        self.change(|state| {
            state.status.phase = if download {
                UpdatePhase::Downloading
            } else {
                UpdatePhase::Checking
            };
            state.status.error = None;
            state.status.downloaded = 0;
            state.status.total = None;
        });
        let updates = self.clone();
        let thread = std::thread::Builder::new()
            .name("update".into())
            .spawn(move || {
                let result = if download {
                    updates.download_inner(&cancellation)
                } else {
                    updates.check_inner(allow_prereleases, &cancellation)
                };
                updates.change(|state| {
                    state.worker = None;
                    if let Err(error) = result {
                        if cancellation.is_cancelled() {
                            state.status.phase = if state.available.is_some() {
                                UpdatePhase::Available
                            } else {
                                UpdatePhase::Idle
                            };
                            state.status.downloaded = 0;
                            state.status.total = None;
                            state.status.error = None;
                        } else {
                            state.status.phase = UpdatePhase::Failed;
                            state.status.error =
                                Some(format!("{error:#}").chars().take(512).collect());
                        }
                    }
                });
            });
        if let Err(error) = thread {
            self.change(|state| {
                state.worker = None;
                state.status.phase = UpdatePhase::Failed;
                state.status.error = Some("Could not start the update worker".into());
            });
            return Err(error.into());
        }
        Ok(())
    }

    fn check_inner(
        &self,
        allow_prereleases: bool,
        cancellation: &http::Cancellation,
    ) -> Result<()> {
        let _apartment = package::Apartment::new()?;
        manifest::public_key()?;
        package::current()?;
        let manifest = discovery::check(allow_prereleases, cancellation)?;
        let current = semver::Version::parse(PRODUCT_VERSION)?;
        let manifest = manifest.filter(|value| {
            manifest::release_version(&value.version).is_ok_and(|version| version > current)
        });
        let obsolete = self.0.state.lock().unwrap().ready.take();
        if let Some(ready) = obsolete {
            let job = ready.job;
            drop(ready);
            helper::clean_job(&self.0.root, job);
        }
        self.change(|state| {
            state.status.phase = if manifest.is_some() {
                UpdatePhase::Available
            } else {
                UpdatePhase::Current
            };
            state.status.version = manifest.as_ref().map(|value| value.version.clone());
            state.available = manifest;
        });
        Ok(())
    }

    fn download_inner(&self, cancellation: &http::Cancellation) -> Result<()> {
        let _apartment = package::Apartment::new()?;
        let key = manifest::public_key()?;
        let current = package::current()?;
        let manifest = self
            .0
            .state
            .lock()
            .unwrap()
            .available
            .clone()
            .context("No available update")?;
        ensure!(
            manifest::mapped_version(&manifest.validate()?) > current.version,
            "The update is no longer newer than the installed version"
        );
        let job = Id::new_v4();
        let directory = self.0.root.join(job.to_string());
        std::fs::create_dir(&directory)?;
        let setup_path = directory.join("setup.exe");
        let path = directory.join("package.msix");
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&setup_path)?;
            let mut last = Instant::now();
            http::download(
                &manifest.artifact().url,
                &mut file,
                manifest::MAX_PACKAGE,
                Duration::from_secs(600),
                cancellation,
                |received, total| {
                    if received == 0
                        || total == Some(received)
                        || last.elapsed() >= Duration::from_millis(100)
                    {
                        self.change(|state| {
                            state.status.downloaded = received;
                            state.status.total = total;
                        });
                        last = Instant::now();
                    }
                },
            )?;
            file.sync_all()?;
            drop(file);
            let mut setup = package::locked_file(&setup_path)?;
            manifest::verify_setup(&mut setup, &manifest, &key, || cancellation.check())?;
            extract_package(&setup_path, cancellation)?;
            let mut file = package::locked_file(&path)?;
            let (bytes, sha256) =
                manifest::verify(&mut file, &manifest, &key, || cancellation.check())?;
            file.seek(SeekFrom::Start(0))?;
            let family = package::validate(&path, &manifest)?;
            ensure!(
                family == current.family,
                "The update targets a different application family"
            );
            cancellation.check()?;
            Ok(Ready {
                job,
                manifest,
                file,
                bytes,
                sha256,
                family,
            })
        })();
        match result {
            Ok(ready) => {
                let previous = self.0.state.lock().unwrap().ready.take();
                if let Some(previous) = previous {
                    let job = previous.job;
                    drop(previous);
                    helper::clean_job(&self.0.root, job);
                }
                self.change(|state| {
                    state.status.phase = UpdatePhase::Ready;
                    state.ready = Some(ready);
                });
                Ok(())
            }
            Err(error) => {
                helper::clean_job(&self.0.root, job);
                Err(error)
            }
        }
    }

    fn change(&self, apply: impl FnOnce(&mut State)) {
        let status = {
            let mut state = self.0.state.lock().unwrap();
            apply(&mut state);
            state.status.revision = state.status.revision.saturating_add(1);
            state.status.clone()
        };
        (self.0.notify)(status);
    }
}

// Only call while holding the authenticated setup file against writes/deletion.
fn extract_package(setup: &Path, cancellation: &http::Cancellation) -> Result<()> {
    cancellation.check()?;
    let mut process = Command::new(setup)
        .args(["/S", "/EXTRACT"])
        .current_dir(setup.parent().context("Missing update directory")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .context("Could not extract the verified update")?;
    let start = Instant::now();
    let result = (|| {
        loop {
            cancellation.check()?;
            ensure!(
                start.elapsed() < Duration::from_secs(60),
                "Update extraction timed out"
            );
            if let Some(status) = process.try_wait()? {
                ensure!(
                    status.success(),
                    "The verified installer could not extract its package"
                );
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    if result.is_err() {
        let _ = process.kill();
        let _ = process.wait();
    }
    result
}
