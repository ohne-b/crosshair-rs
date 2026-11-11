use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use crosshair_core::{
    AppState, Asset, AssetKind, Content, Id, Image, Layer, MAX_ASSET_BYTES, Preset,
};
use crosshair_renderer::{AssetData, Renderer, inspect_asset};
use sha2::{Digest, Sha256};

use super::{
    Store,
    state::{open_read, read_bounded, regular_path},
};

pub(super) struct AssetRead {
    id: Id,
    metadata: Asset,
    bytes: Arc<[u8]>,
}

pub fn read_asset_at(root: &Path, assets: &BTreeMap<Id, Asset>, id: Id) -> Result<AssetData> {
    let asset = assets
        .get(&id)
        .context("Image asset is not in this document")?;
    regular_path(&root.join("assets"), true)?;
    let bytes = read_bounded(
        open_read(&root.join("assets").join(id.to_string()))?,
        MAX_ASSET_BYTES,
    )?;
    ensure!(
        bytes.len() as u64 == asset.source_bytes
            && format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
        "Stored image is missing or changed; restore it from a backup"
    );
    Ok(AssetData {
        kind: asset.kind.clone(),
        bytes: Arc::from(bytes),
    })
}

pub(super) fn inspect(bytes: Arc<[u8]>, renderer: &mut Renderer) -> Result<(AssetData, Asset)> {
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_ASSET_BYTES,
        "Image must contain at most 8 MiB"
    );
    let kind = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        AssetKind::Png
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        AssetKind::Jpeg
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        AssetKind::Gif
    } else {
        AssetKind::Svg
    };
    let data = AssetData {
        kind: kind.clone(),
        bytes,
    };
    let info = inspect_asset(&data)?;
    let asset = Asset {
        sha256: format!("{:x}", Sha256::digest(data.bytes.as_ref())),
        kind,
        source_bytes: data.bytes.len() as u64,
        width: info.width,
        height: info.height,
    };
    let id = Id::new_v4();
    let fit = 64.0 / info.width.max(info.height) as f32;
    let mut preset = Preset::new("Image validation");
    preset.layers = vec![Layer::new(
        "Image",
        Content::Image(Image::new(
            id,
            info.width as f32 * fit,
            info.height as f32 * fit,
        )),
    )];
    renderer.render(&preset, 1.0, &|requested| {
        ensure!(requested == id, "Unexpected validation asset");
        Ok(data.clone())
    })?;
    Ok((data, asset))
}

impl Store {
    pub fn read_asset(&self, state: &AppState, id: Id) -> Result<AssetData> {
        read_asset_at(&self.root, &state.assets, id)
    }

    pub fn clear_read_cache(&self) {
        *self.asset_chunk.borrow_mut() = None;
    }

    pub(super) fn read_asset_metadata(&self, id: Id, asset: &Asset) -> Result<AssetData> {
        let bytes = read_bounded(open_read(&self.asset_path(id))?, MAX_ASSET_BYTES)?;
        ensure!(
            bytes.len() as u64 == asset.source_bytes
                && format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
            "Stored image is missing or changed; restore it from a backup"
        );
        Ok(AssetData {
            kind: asset.kind.clone(),
            bytes: Arc::from(bytes),
        })
    }

    pub fn read_asset_chunk(
        &self,
        state: &AppState,
        id: Id,
        offset: u64,
    ) -> Result<(Vec<u8>, bool)> {
        let metadata = state
            .assets
            .get(&id)
            .context("Image asset is not in this document")?;
        let mut cached = self.asset_chunk.borrow_mut();
        if offset == 0
            || cached
                .as_ref()
                .is_none_or(|cached| cached.id != id || cached.metadata != *metadata)
        {
            let data = self.read_asset(state, id)?;
            *cached = Some(AssetRead {
                id,
                metadata: metadata.clone(),
                bytes: data.bytes,
            });
        }
        let data = cached.as_ref().expect("asset snapshot was initialized");
        ensure!(
            offset <= data.bytes.len() as u64,
            "Asset offset exceeds its size"
        );
        let end = (offset as usize + crosshair_core::protocol::CHUNK_BYTES).min(data.bytes.len());
        Ok((
            data.bytes[offset as usize..end].to_vec(),
            end == data.bytes.len(),
        ))
    }

    pub(super) fn verify_assets(&self, state: &AppState, hashes: bool) -> Result<()> {
        regular_path(&self.root.join("assets"), true)?;
        for (&id, asset) in &state.assets {
            if hashes {
                let data = self.read_asset_metadata(id, asset)?;
                // SVG fonts may have been uninstalled since the original import.
                // Keep the library editable and let rendering report that error;
                // immutable source hashes still verify the stored SVG bytes.
                if asset.kind != AssetKind::Svg {
                    let info = inspect_asset(&data)?;
                    ensure!(
                        info.width == asset.width && info.height == asset.height,
                        "Stored image dimensions do not match its metadata"
                    );
                }
            } else {
                let file =
                    open_read(&self.asset_path(id)).context("A referenced image is missing")?;
                ensure!(
                    file.metadata()?.len() == asset.source_bytes,
                    "A referenced image has changed"
                );
            }
        }
        Ok(())
    }

    pub(super) fn adopt(&self, id: Id, data: &AssetData) -> Result<()> {
        #[cfg(test)]
        if let Some(remaining) = self.fail_adoption_after.get() {
            ensure!(remaining > 0, "Injected adoption failure");
            self.fail_adoption_after.set(Some(remaining - 1));
        }
        regular_path(&self.root.join("assets"), true)?;
        let path = self.asset_path(id);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            file.write_all(&data.bytes)?;
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(&path);
        } else {
            self.pending_assets.borrow_mut().insert(id, None);
        }
        result
    }
}
