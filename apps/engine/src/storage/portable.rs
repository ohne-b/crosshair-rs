use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD, write::EncoderWriter};
use crosshair_core::{
    AppState, Asset, BindingAction, Content, Id, MAX_ASSET_BYTES, MAX_CONFIG_BYTES,
    MAX_PORTABLE_BYTES, Profile, Settings, protocol::ExportKind, validate_state,
};
use crosshair_renderer::{AssetData, Renderer};
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};

use super::{
    Store, assets,
    state::{atomic_write, open_read, read_bounded},
};

const FORMAT: &str = "crosshair-rs";
const VERSION: u32 = 1;
const RECORD_LIMIT: u64 = MAX_ASSET_BYTES.div_ceil(3) * 4 + 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Preset,
    Profile,
    All,
}

struct Imported {
    kind: Kind,
    state: AppState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedded {
    id: Id,
    data: String,
}

#[derive(Default)]
struct StagedAssets {
    files: BTreeMap<Id, (PathBuf, Asset)>,
}
impl Drop for StagedAssets {
    fn drop(&mut self) {
        for (path, _) in self.files.values() {
            let _ = fs::remove_file(path);
        }
    }
}

impl Store {
    pub fn export(
        &self,
        state: &AppState,
        kind: ExportKind,
        destination: &Path,
        renderer: &mut Renderer,
    ) -> Result<()> {
        let destination = self.export_destination(destination)?;
        if matches!(kind, ExportKind::Recovery) && self.load().is_err() {
            return atomic_write(&destination, |writer| {
                let mut writer = LimitWriter { writer, bytes: 0 };
                writer.write_all(
                    b"{\"format\":\"crosshair-rs-recovery\",\"version\":1,\"ram_state\":",
                )?;
                validate_state(state)?;
                serde_json::to_writer(&mut writer, state)?;
                writer.write_all(b",\"files\":[")?;
                for (index, (name, path, limit)) in self.recovery_files()?.into_iter().enumerate() {
                    if index > 0 {
                        writer.write_all(b",")?;
                    }
                    let mut file = open_read(&path)?;
                    ensure!(
                        file.metadata()?.len() <= limit,
                        "Recovery source exceeds its size limit; originals were preserved"
                    );
                    writer.write_all(b"{\"path\":")?;
                    serde_json::to_writer(&mut writer, &name)?;
                    writer.write_all(b",\"data\":\"")?;
                    {
                        let mut encoder = EncoderWriter::new(&mut writer, &STANDARD);
                        io::copy(&mut file, &mut encoder)?;
                        encoder.finish()?;
                    }
                    writer.write_all(b"\"}")?;
                }
                writer.write_all(b"]}")?;
                Ok(())
            });
        }
        if let ExportKind::Png { preset } | ExportKind::Svg { preset } = kind {
            let preset = state.preset(preset).context("Preset does not exist")?;
            let bytes = if matches!(kind, ExportKind::Svg { .. }) {
                renderer.to_svg(preset, 1.0, &|id| self.read_asset(state, id))?
            } else {
                renderer
                    .render(preset, 1.0, &|id| self.read_asset(state, id))?
                    .to_png()?
            };
            return atomic_write(&destination, |writer| {
                writer.write_all(&bytes)?;
                Ok(())
            });
        }
        let (kind, state) = select_export(state, kind)?;
        atomic_write(&destination, |writer| {
            write_portable(self, &state, kind, writer)
        })
    }

    pub fn create_backup(&self, state: &AppState) -> Result<PathBuf> {
        super::state::regular_path(&self.root.join("backups"), true)?;
        let destination = self
            .root
            .join("backups")
            .join(format!("data-{}.json", Id::new_v4()));
        atomic_write(&destination, |writer| {
            write_portable(self, state, Kind::All, writer)
        })?;
        Ok(destination)
    }
}

fn select_export(state: &AppState, kind: ExportKind) -> Result<(Kind, AppState)> {
    validate_state(state)?;
    let mut result = state.clone();
    let kind = match kind {
        ExportKind::All | ExportKind::Recovery => return Ok((Kind::All, result)),
        ExportKind::Preset { preset } => {
            result.presets = vec![
                state
                    .preset(preset)
                    .context("Preset does not exist")?
                    .clone(),
            ];
            result.profiles = vec![Profile::new("Imported", preset)];
            Kind::Preset
        }
        ExportKind::Profile { profile } => {
            let profile = state
                .profiles
                .iter()
                .find(|candidate| candidate.id == profile)
                .context("Profile does not exist")?
                .clone();
            let ids: BTreeSet<_> = std::iter::once(profile.selected_preset)
                .chain(
                    profile
                        .bindings
                        .iter()
                        .filter_map(|binding| binding.action.target()),
                )
                .collect();
            result.presets.retain(|preset| ids.contains(&preset.id));
            result.profiles = vec![profile];
            Kind::Profile
        }
        ExportKind::Png { .. } | ExportKind::Svg { .. } => {
            unreachable!("Image export has a separate encoder")
        }
    };
    result.active_profile = result.profiles[0].id;
    result.settings = Settings {
        global_toggle: None,
        ..Default::default()
    };
    result.base_visible = true;
    result.revision = 0;
    let assets: BTreeSet<_> = result
        .presets
        .iter()
        .flat_map(|preset| &preset.layers)
        .filter_map(|layer| layer.content.asset_id())
        .collect();
    result.assets.retain(|id, _| assets.contains(id));
    validate_state(&result)?;
    Ok((kind, result))
}

fn write_portable(store: &Store, state: &AppState, kind: Kind, writer: &mut File) -> Result<()> {
    validate_state(state)?;
    let mut writer = LimitWriter { writer, bytes: 0 };
    writer.write_all(b"{\"format\":\"crosshair-rs\",\"version\":1,\"kind\":")?;
    serde_json::to_writer(&mut writer, &kind)?;
    writer.write_all(b",\"state\":")?;
    serde_json::to_writer(&mut writer, state)?;
    writer.write_all(b",\"assets\":[")?;
    for (index, &id) in state.assets.keys().enumerate() {
        if index > 0 {
            writer.write_all(b",")?;
        }
        let data = store.read_asset(state, id)?;
        writer.write_all(b"{\"id\":")?;
        serde_json::to_writer(&mut writer, &id)?;
        writer.write_all(b",\"data\":\"")?;
        {
            let mut encoder = EncoderWriter::new(&mut writer, &STANDARD);
            encoder.write_all(&data.bytes)?;
            encoder.finish()?;
        }
        writer.write_all(b"\"}")?;
    }
    writer.write_all(b"]}")?;
    Ok(())
}

struct LimitWriter<'a> {
    writer: &'a mut File,
    bytes: u64,
}
impl Write for LimitWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes.saturating_add(buffer.len() as u64) > MAX_PORTABLE_BYTES {
            return Err(io::Error::other("Portable JSON exceeds 512 MiB"));
        }
        let count = self.writer.write(buffer)?;
        self.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub(super) fn import(
    store: &Store,
    file: File,
    current: &AppState,
    replace: bool,
    renderer: &mut Renderer,
) -> Result<AppState> {
    ensure!(
        file.metadata()?.len() <= MAX_PORTABLE_BYTES,
        "Portable JSON exceeds 512 MiB"
    );
    let quota = Rc::new(Cell::new(MAX_CONFIG_BYTES as u64 + 1024));
    let reader = LimitReader {
        reader: BufReader::new(file),
        component: quota.clone(),
        total: 0,
    };
    let mut decoder = serde_json::Deserializer::from_reader(reader);
    let mut staged = StagedAssets::default();
    let imported = PortableSeed {
        store,
        renderer,
        quota: quota.clone(),
        staged: &mut staged,
    }
    .deserialize(&mut decoder)?;
    quota.set(1024);
    decoder.end()?;
    validate_state(&imported.state)?;
    ensure!(
        !replace || matches!(imported.kind, Kind::All),
        "Replace requires an all-data backup"
    );
    match imported.kind {
        Kind::Preset => ensure!(
            imported.state.presets.len() == 1 && imported.state.profiles.len() == 1,
            "Preset export contains unrelated documents"
        ),
        Kind::Profile => ensure!(
            imported.state.profiles.len() == 1,
            "Profile export contains multiple profiles"
        ),
        Kind::All => {}
    }
    ensure!(
        staged.files.len() == imported.state.assets.len(),
        "Embedded assets do not match the document index"
    );
    for (id, metadata) in &imported.state.assets {
        ensure!(
            staged
                .files
                .get(id)
                .is_some_and(|(_, actual)| actual == metadata),
            "Embedded image metadata or checksum does not match its content"
        );
    }
    let (candidate, mappings) = merge(current, imported.state, imported.kind, replace)?;
    validate_state(&candidate)?;
    // Check each reachable scale: floor/ceil pixel bounds can differ even for
    // almost-equal scales when a transformed origin lands between pixels.
    let scales: BTreeSet<_> = candidate
        .profiles
        .iter()
        .flat_map(|profile| {
            std::iter::once(profile.scale)
                .chain(profile.positions.iter().map(|position| position.scale))
        })
        .map(f32::to_bits)
        .chain(std::iter::once(1.0f32.to_bits()))
        .collect();
    let source = |id| -> Result<AssetData> {
        if current.assets.contains_key(&id) {
            return store.read_asset(current, id);
        }
        let original = mappings
            .iter()
            .find_map(|(old, new)| (*new == id).then_some(old))
            .context("Imported asset mapping is missing")?;
        let (path, metadata) = staged
            .files
            .get(original)
            .context("Imported asset bytes are missing")?;
        Ok(AssetData {
            kind: metadata.kind.clone(),
            bytes: Arc::from(read_bounded(open_read(path)?, MAX_ASSET_BYTES)?),
        })
    };
    for preset in &candidate.presets {
        for scale in &scales {
            renderer.render(preset, f32::from_bits(*scale), &source)?;
            renderer.clear_raster_cache();
        }
    }
    if replace {
        store.create_backup(current)?;
    }
    let mut adopted = Vec::new();
    let result = (|| {
        for (old, new) in mappings {
            if current.assets.contains_key(&new) {
                store.read_asset(current, new)?;
                continue;
            }
            let (path, metadata) = staged
                .files
                .get(&old)
                .context("Imported asset bytes are missing")?;
            let data = AssetData {
                kind: metadata.kind.clone(),
                bytes: Arc::from(read_bounded(open_read(path)?, MAX_ASSET_BYTES)?),
            };
            store.adopt(new, &data)?;
            adopted.push(new);
        }
        Ok(candidate)
    })();
    if result.is_err() {
        for id in adopted {
            store.pending_assets.borrow_mut().remove(&id);
            let _ = fs::remove_file(store.asset_path(id));
        }
    }
    result
}

fn merge(
    current: &AppState,
    mut imported: AppState,
    kind: Kind,
    replace: bool,
) -> Result<(AppState, BTreeMap<Id, Id>)> {
    let mut assets = BTreeMap::new();
    let mut mappings = BTreeMap::new();
    for (&old, asset) in &imported.assets {
        let id = if let Some((&id, existing)) = current
            .assets
            .iter()
            .find(|(_, candidate)| candidate.sha256 == asset.sha256)
        {
            ensure!(existing == asset, "Duplicate image metadata does not match");
            id
        } else {
            Id::new_v4()
        };
        mappings.insert(old, id);
        assets.insert(id, asset.clone());
    }
    let presets: HashMap<_, _> = imported
        .presets
        .iter()
        .map(|preset| (preset.id, Id::new_v4()))
        .collect();
    for preset in &mut imported.presets {
        preset.id = presets[&preset.id];
        preset.revision = 0;
        preset.modified_order = current
            .revision
            .checked_add(1)
            .context("Revision counter is exhausted")?;
        for layer in &mut preset.layers {
            layer.id = Id::new_v4();
            let image = match &mut layer.content {
                Content::Image(image) => Some(image),
                Content::Crosshair(crosshair) => {
                    if let crosshair_core::ArmShape::Image(image) = &mut crosshair.shape {
                        Some(image)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(image) = image {
                image.asset = *mappings
                    .get(&image.asset)
                    .context("Image reference is missing")?;
            }
        }
    }
    for profile in &mut imported.profiles {
        let old = profile.id;
        profile.id = Id::new_v4();
        if imported.active_profile == old {
            imported.active_profile = profile.id;
        }
        profile.selected_preset = *presets
            .get(&profile.selected_preset)
            .context("Selected preset reference is missing")?;
        for position in &mut profile.positions {
            position.id = Id::new_v4();
        }
        for binding in &mut profile.bindings {
            binding.id = Id::new_v4();
            if let BindingAction::SelectPreset { preset } = &mut binding.action {
                *preset = *presets.get(preset).context("Binding target is missing")?;
            }
        }
    }
    imported.assets = assets;
    let mut candidate = if replace {
        imported
    } else {
        let mut result = current.clone();
        result.presets.extend(imported.presets);
        if !matches!(kind, Kind::Preset) {
            result.profiles.extend(imported.profiles);
        }
        result.assets.extend(imported.assets);
        result
    };
    candidate.revision = current
        .revision
        .checked_add(1)
        .context("Revision counter is exhausted")?;
    Ok((candidate, mappings))
}

struct LimitReader<R> {
    reader: R,
    component: Rc<Cell<u64>>,
    total: u64,
}
impl<R: Read> Read for LimitReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let available = self
            .component
            .get()
            .min(MAX_PORTABLE_BYTES.saturating_sub(self.total));
        if available == 0 {
            let mut extra = [0u8; 1];
            if self.reader.read(&mut extra)? == 0 {
                return Ok(0);
            }
            return Err(io::Error::other(
                "JSON component or document exceeds its size limit",
            ));
        }
        let length = bytes.len().min(available as usize);
        let count = self.reader.read(&mut bytes[..length])?;
        self.total += count as u64;
        self.component.set(self.component.get() - count as u64);
        Ok(count)
    }
}

struct PortableSeed<'a> {
    store: &'a Store,
    renderer: &'a mut Renderer,
    quota: Rc<Cell<u64>>,
    staged: &'a mut StagedAssets,
}
impl<'de> DeserializeSeed<'de> for PortableSeed<'_> {
    type Value = Imported;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for PortableSeed<'_> {
    type Value = Imported;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a versioned crosshair-rs portable document")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Imported, M::Error> {
        let mut format = None;
        let mut version = None;
        let mut kind = None;
        let mut state = None;
        let mut assets_seen = false;
        loop {
            self.quota.set(MAX_CONFIG_BYTES as u64 + 1024);
            let Some(key) = map.next_key::<String>()? else {
                break;
            };
            match key.as_str() {
                "format" if format.is_none() => format = Some(map.next_value::<String>()?),
                "version" if version.is_none() => version = Some(map.next_value::<u32>()?),
                "kind" if kind.is_none() => kind = Some(map.next_value::<Kind>()?),
                "state" if state.is_none() => state = Some(map.next_value::<AppState>()?),
                "assets" if !assets_seen => {
                    assets_seen = true;
                    map.next_value_seed(AssetSeed {
                        store: self.store,
                        renderer: self.renderer,
                        quota: self.quota.clone(),
                        staged: self.staged,
                    })?;
                }
                _ => {
                    return Err(de::Error::custom(
                        "Unknown or duplicate portable document field",
                    ));
                }
            }
        }
        if format.as_deref() != Some(FORMAT) || version != Some(VERSION) {
            return Err(de::Error::custom(
                "Unsupported portable document format or version",
            ));
        }
        if !assets_seen {
            return Err(de::Error::missing_field("assets"));
        }
        Ok(Imported {
            kind: kind.ok_or_else(|| de::Error::missing_field("kind"))?,
            state: state.ok_or_else(|| de::Error::missing_field("state"))?,
        })
    }
}

struct AssetSeed<'a> {
    store: &'a Store,
    renderer: &'a mut Renderer,
    quota: Rc<Cell<u64>>,
    staged: &'a mut StagedAssets,
}
impl<'de> DeserializeSeed<'de> for AssetSeed<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<(), D::Error> {
        decoder.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for AssetSeed<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a bounded sequence of embedded images")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> std::result::Result<(), S::Error> {
        let mut metadata_bytes = 0usize;
        loop {
            self.quota.set(RECORD_LIMIT);
            let Some(record) = sequence.next_element::<Embedded>()? else {
                break;
            };
            let result = (|| -> Result<()> {
                ensure!(
                    !record.id.is_nil() && !self.staged.files.contains_key(&record.id),
                    "Duplicate or invalid embedded asset ID"
                );
                ensure!(
                    record.data.len() as u64 <= MAX_ASSET_BYTES.div_ceil(3) * 4,
                    "Embedded image exceeds 8 MiB"
                );
                let bytes = STANDARD.decode(record.data)?;
                let (data, metadata) = assets::inspect(Arc::from(bytes), self.renderer)?;
                metadata_bytes += serde_json::to_vec(&metadata)?.len() + 64;
                ensure!(
                    metadata_bytes <= MAX_CONFIG_BYTES,
                    "Embedded asset index exceeds 8 MiB"
                );
                let path = self.store.staging_path(Id::new_v4());
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                self.staged.files.insert(record.id, (path, metadata));
                file.write_all(&data.bytes)?;
                file.sync_all()?;
                Ok(())
            })();
            result.map_err(de::Error::custom)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn svg_export_is_self_contained_and_failed_export_preserves_destination() -> Result<()> {
        let data = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let store = Store::open(data.path())?;
        let mut renderer = Renderer::new()?;
        let mut state = AppState::default();
        let path = output.path().join("crosshair.svg");
        let kind = ExportKind::Svg {
            preset: state.presets[0].id,
        };
        store.export(&state, kind.clone(), &path, &mut renderer)?;
        let exported = fs::read(&path)?;
        let svg = std::str::from_utf8(&exported)?;
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<path"));
        assert!(!svg.contains("<image"));
        assert!(!svg.contains("<text"));
        state.presets[0].layers[0].content = Content::Rectangle(crosshair_core::Rectangle {
            width: 1024.0,
            height: 2.0,
            ..Default::default()
        });
        state.presets[0].layers[0].effects.blur = 1.0;
        assert!(store.export(&state, kind, &path, &mut renderer).is_err());
        assert_eq!(fs::read(&path)?, exported);
        assert!(
            store
                .export(
                    &state,
                    ExportKind::Svg {
                        preset: Id::new_v4()
                    },
                    &path,
                    &mut renderer
                )
                .is_err()
        );
        assert_eq!(fs::read(&path)?, exported);
        Ok(())
    }
}
