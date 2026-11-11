use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use crosshair_core::{AppState, Id, MAX_CONFIG_BYTES, validate_state};

pub fn default_data_directory() -> Result<PathBuf> {
    #[cfg(windows)]
    if let Ok(data) = windows::Storage::ApplicationData::Current() {
        return Ok(PathBuf::from(data.LocalFolder()?.Path()?.to_string()));
    }
    Ok(PathBuf::from(
        std::env::var_os("LOCALAPPDATA").context("Windows local app data is unavailable")?,
    )
    .join("crosshair-rs"))
}

pub struct Store {
    pub(super) root: PathBuf,
    pub(super) staging: HashMap<Id, super::staging::Lease>,
    pub(super) asset_chunk: std::cell::RefCell<Option<super::assets::AssetRead>>,
    pub(super) pending_assets: std::cell::RefCell<HashMap<Id, Option<Id>>>,
    #[cfg(test)]
    pub(super) fail_adoption_after: std::cell::Cell<Option<usize>>,
    // An exclusive OS handle protects the same user store across Windows sessions.
    _lock: File,
}

impl Drop for Store {
    fn drop(&mut self) {
        for &token in self.staging.keys() {
            let _ = fs::remove_file(self.staging_path(token));
        }
    }
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref()).context("Could not create the data directory")?;
        let root = root.as_ref().canonicalize()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let lock = options.open(root.join("state.lock")).context(
            "crosshair-rs is already using this data in another process or Windows session",
        )?;
        for name in ["assets", "staging", "backups"] {
            let path = root.join(name);
            fs::create_dir_all(&path)?;
            regular_path(&path, true)?;
        }
        for entry in fs::read_dir(root.join("staging"))? {
            let entry = entry?;
            if Id::parse_str(&entry.file_name().to_string_lossy()).is_ok()
                && regular_path(&entry.path(), false).is_ok()
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(Self {
            root,
            staging: HashMap::new(),
            asset_chunk: std::cell::RefCell::new(None),
            pending_assets: std::cell::RefCell::new(HashMap::new()),
            #[cfg(test)]
            fail_adoption_after: std::cell::Cell::new(None),
            _lock: lock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load(&self) -> Result<AppState> {
        let path = self.root.join("state.json");
        if !path.try_exists()? {
            let evidence = self.root.join("state.backup.json").try_exists()?
                || self.root.join("initialized").try_exists()?
                || fs::read_dir(self.root.join("assets"))?.next().is_some()
                || fs::read_dir(self.root.join("backups"))?.next().is_some()
                || fs::read_dir(&self.root)?.any(|entry| {
                    entry.is_ok_and(|entry| {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        name.starts_with("state-recovery-") || name.starts_with("recovery-")
                    })
                });
            ensure!(
                !evidence,
                "Settings are missing from an existing installation; recover a backup instead of creating defaults"
            );
            return Ok(AppState::default());
        }
        let state = read_state(open_read(&path)?)
            .context("Settings could not be read. The original and backup were preserved")?;
        self.verify_assets(&state, true)?;
        Ok(state)
    }

    pub fn backup_state(&self) -> Result<AppState> {
        let state = read_state(open_read(&self.root.join("state.backup.json"))?)?;
        self.verify_assets(&state, true)?;
        Ok(state)
    }

    pub fn save(&self, state: &AppState) -> Result<()> {
        validate_state(state)?;
        self.verify_assets(state, false)?;
        let bytes = serde_json::to_vec(state)?;
        ensure!(
            bytes.len() <= MAX_CONFIG_BYTES,
            "Settings exceed the storage limit"
        );
        let destination = self.root.join("state.json");
        if destination.try_exists()? {
            let old = read_state(open_read(&destination)?)
                .context("Current settings need recovery before saving")?;
            self.verify_assets(&old, false)?;
            atomic_write(&self.root.join("state.backup.json"), |writer| {
                serde_json::to_writer(writer, &old)?;
                Ok(())
            })?;
        }
        if !self.root.join("initialized").try_exists()? {
            atomic_write(&self.root.join("initialized"), |writer| {
                writer.write_all(b"1\n")?;
                Ok(())
            })?;
        }
        atomic_write(&destination, |writer| {
            writer.write_all(&bytes)?;
            Ok(())
        })
        .context("Settings could not be saved; the last durable state is preserved")?;
        self.pending_assets
            .borrow_mut()
            .retain(|id, _| !state.assets.contains_key(id));
        Ok(())
    }

    #[cfg(test)]
    pub fn recover_backup(&self) -> Result<AppState> {
        let state = self.prepare_recovery()?;
        self.install_recovery(&state)?;
        Ok(state)
    }

    pub fn prepare_recovery(&self) -> Result<AppState> {
        let state = self.backup_state()?;
        let current = self.root.join("state.json");
        if current.exists() {
            let preserved = self
                .root
                .join(format!("state-recovery-{}.json", Id::new_v4()));
            let mut file = open_read(&current)?;
            atomic_write(&preserved, |writer| {
                std::io::copy(&mut file, writer)?;
                Ok(())
            })?;
        }
        Ok(state)
    }

    pub fn asset_path(&self, id: Id) -> PathBuf {
        self.root.join("assets").join(id.to_string())
    }
    pub(super) fn staging_path(&self, id: Id) -> PathBuf {
        self.root.join("staging").join(id.to_string())
    }

    pub fn collect_assets(&self, durable: &AppState, history: &BTreeSet<Id>) -> Result<usize> {
        let live = read_state(open_read(&self.root.join("state.json"))?)?;
        ensure!(
            &live == durable,
            "Asset cleanup requires the current durable snapshot"
        );
        let mut keep: BTreeSet<Id> = durable
            .assets
            .keys()
            .copied()
            .chain(history.iter().copied())
            .collect();
        keep.extend(self.pending_assets.borrow().keys().copied());
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "state.backup.json" || name.starts_with("state-recovery-") {
                let state = read_state(open_read(&entry.path())?)
                    .context("Asset cleanup deferred while a preserved snapshot needs recovery")?;
                keep.extend(state.assets.keys().copied());
            }
        }
        let mut removed = 0;
        for entry in fs::read_dir(self.root.join("assets"))? {
            let entry = entry?;
            if let Ok(id) = Id::parse_str(&entry.file_name().to_string_lossy())
                && !keep.contains(&id)
            {
                regular_path(&entry.path(), false)?;
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub(super) fn export_destination(&self, destination: &Path) -> Result<PathBuf> {
        let name = destination
            .file_name()
            .context("Choose an export filename")?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .context("Export directory does not exist")?;
        ensure!(
            !parent.starts_with(&self.root),
            "Exports cannot overwrite managed application data"
        );
        let resolved = parent.join(name);
        if resolved.try_exists()? {
            regular_path(&resolved, false)?;
            ensure!(
                !resolved.canonicalize()?.starts_with(&self.root),
                "Exports cannot overwrite managed application data"
            );
        }
        Ok(resolved)
    }
}

pub(super) fn read_state(file: File) -> Result<AppState> {
    ensure!(
        file.metadata()?.len() <= MAX_CONFIG_BYTES as u64,
        "Settings exceed the storage limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_CONFIG_BYTES,
        "Settings grew beyond the storage limit"
    );
    let state: AppState = serde_json::from_slice(&bytes)?;
    validate_state(&state)?;
    Ok(state)
}

pub(super) fn regular_path(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "Expected a regular managed file or directory"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Reparse points are not allowed in managed storage"
        );
    }
    ensure!(
        !metadata.file_type().is_symlink(),
        "Symbolic links are not allowed in managed storage"
    );
    Ok(())
}

pub(super) fn open_read(path: &Path) -> Result<File> {
    regular_path(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            file.metadata()?.file_attributes() & 0x400 == 0,
            "Managed file became a reparse point"
        );
    }
    Ok(file)
}

pub(super) fn read_bounded(file: File, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        file.metadata()?.len() <= limit,
        "File exceeds the supported size limit"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "File grew beyond the supported size limit"
    );
    Ok(bytes)
}

pub(super) fn atomic_write(
    destination: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    let temporary = destination.with_file_name(format!(".crosshair-{}.tmp", Id::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace(temporary: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACE_FILE_FLAGS, ReplaceFileW,
            },
            core::PCWSTR,
        };
        let wide = |path: &Path| {
            path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let source = wide(temporary);
        let target = wide(destination);
        unsafe {
            if destination.exists() {
                ReplaceFileW(
                    PCWSTR(target.as_ptr()),
                    PCWSTR(source.as_ptr()),
                    None,
                    REPLACE_FILE_FLAGS(0),
                    None,
                    None,
                )?;
            } else {
                MoveFileExW(
                    PCWSTR(source.as_ptr()),
                    PCWSTR(target.as_ptr()),
                    MOVEFILE_WRITE_THROUGH,
                )?;
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn immutable_readers_share_access_but_reject_writes_and_replacement() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("asset");
        fs::write(&path, b"immutable")?;
        let first = open_read(&path)?;
        let second = open_read(&path)?;
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, directory.path().join("moved")).is_err());
        drop(first);
        drop(second);
        assert!(OpenOptions::new().write(true).open(&path).is_ok());
        Ok(())
    }

    #[test]
    fn repeated_atomic_save_retains_previous_valid_state() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        let mut state = store.load()?;
        store.save(&state)?;
        state.base_visible = false;
        state.revision += 1;
        store.save(&state)?;
        assert!(!store.load()?.base_visible);
        assert!(store.backup_state()?.base_visible);
        state.base_visible = true;
        state.revision += 1;
        store.save(&state)?;
        assert!(!store.backup_state()?.base_visible);
        assert!(store.load()?.base_visible);
        Ok(())
    }

    #[test]
    fn corrupt_or_future_settings_never_turn_into_defaults() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        fs::write(directory.path().join("state.json"), b"{broken")?;
        assert!(store.load().is_err());
        assert_eq!(fs::read(directory.path().join("state.json"))?, b"{broken");
        let mut state = AppState::default();
        state.schema_version += 1;
        assert!(store.save(&state).is_err());
        Ok(())
    }

    #[test]
    #[cfg(windows)]
    fn second_writer_is_refused_until_first_handle_closes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let first = Store::open(directory.path())?;
        assert!(Store::open(directory.path()).is_err());
        drop(first);
        assert!(Store::open(directory.path()).is_ok());
        Ok(())
    }
}
