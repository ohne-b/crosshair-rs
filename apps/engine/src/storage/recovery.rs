use std::{fs, path::PathBuf};

use anyhow::{Result, ensure};
use crosshair_core::{
    AppState, Id, MAX_ASSET_BYTES, MAX_CONFIG_BYTES, MAX_PORTABLE_BYTES, validate_state,
};

use super::{
    Store,
    state::{atomic_write, open_read, regular_path},
};

impl Store {
    pub(super) fn recovery_files(&self) -> Result<Vec<(String, PathBuf, u64)>> {
        let mut files = Vec::new();
        for name in ["state.json", "state.backup.json"] {
            let path = self.root.join(name);
            if path.try_exists()? {
                regular_path(&path, false)?;
                files.push((name.into(), path, MAX_CONFIG_BYTES as u64));
            }
        }
        regular_path(&self.root.join("assets"), true)?;
        for entry in fs::read_dir(self.root.join("assets"))? {
            let entry = entry?;
            if let Ok(id) = Id::parse_str(&entry.file_name().to_string_lossy()) {
                regular_path(&entry.path(), false)?;
                files.push((format!("assets/{id}"), entry.path(), MAX_ASSET_BYTES));
            }
        }
        ensure!(
            files.len() <= 40_000,
            "Recovery contains too many source files; originals were preserved"
        );
        Ok(files)
    }

    /// Retains original bytes even when their JSON or image content is corrupt.
    pub fn preserve_recovery(&self) -> Result<PathBuf> {
        let files = self.recovery_files()?;
        let destination = self.root.join(format!("recovery-{}", Id::new_v4()));
        fs::create_dir(&destination)?;
        fs::create_dir(destination.join("assets"))?;
        let mut copied = Vec::new();
        let result = (|| {
            let mut total = 0u64;
            for (name, source, limit) in files {
                let mut file = open_read(&source)?;
                let length = file.metadata()?.len();
                total = total.saturating_add(length);
                ensure!(
                    length <= limit && total <= MAX_PORTABLE_BYTES,
                    "Recovery sources exceed the bounded backup limit; originals were preserved"
                );
                let target = destination.join(name);
                atomic_write(&target, |writer| {
                    std::io::copy(&mut file, writer)?;
                    Ok(())
                })?;
                copied.push(target);
            }
            Ok(destination.clone())
        })();
        if result.is_err() {
            for file in copied {
                let _ = fs::remove_file(file);
            }
            let _ = fs::remove_dir(destination.join("assets"));
            let _ = fs::remove_dir(&destination);
        }
        result
    }

    /// Explicit reset is the only operation allowed to replace unreadable live
    /// settings, and only after both durable bytes and pending RAM data are safe.
    #[cfg(test)]
    pub fn reset(&self, current: &AppState) -> Result<AppState> {
        let state = self.prepare_reset(current)?;
        self.install_recovery(&state)?;
        Ok(state)
    }

    pub fn prepare_reset(&self, current: &AppState) -> Result<AppState> {
        validate_state(current)?;
        let preserved = self.preserve_recovery()?;
        atomic_write(&preserved.join("ram-state.json"), |writer| {
            serde_json::to_writer(writer, current)?;
            Ok(())
        })?;
        // Missing/corrupt sources cannot form a valid portable backup. The raw
        // directory plus RAM metadata preserves everything that still exists.
        if self.verify_assets(current, true).is_ok() {
            self.create_backup(current)?;
        }
        let state = AppState {
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Revision counter is exhausted"))?,
            ..Default::default()
        };
        Ok(state)
    }

    pub fn install_recovery(&self, state: &AppState) -> Result<()> {
        validate_state(state)?;
        self.verify_assets(state, true)?;
        atomic_write(&self.root.join("state.json"), |writer| {
            serde_json::to_writer(writer, state)?;
            Ok(())
        })
    }
}
