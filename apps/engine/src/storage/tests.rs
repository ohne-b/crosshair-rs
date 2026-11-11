use std::{collections::BTreeSet, fs, io::Write, path::Path};

use anyhow::Result;
use crosshair_core::{
    AppState, Binding, BindingAction, Content, Id, Image, InputChord, InputKey, MAX_PORTABLE_BYTES,
    Point, SavedPosition,
    protocol::{ExportKind, StagingPurpose},
};
use crosshair_renderer::Renderer;

use super::{Staged, Store};

#[test]
fn preparing_a_reset_preserves_live_state_until_output_is_ready() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let mut current = AppState::default();
    current.presets[0].name = "Keep until committed".into();
    store.save(&current)?;
    let prepared = store.prepare_reset(&current)?;
    assert_eq!(store.load()?, current);
    assert_ne!(prepared.presets[0].id, current.presets[0].id);
    store.install_recovery(&prepared)?;
    assert_eq!(store.load()?, prepared);
    Ok(())
}

fn add_image(
    store: &mut Store,
    state: &mut AppState,
    renderer: &mut Renderer,
    color: &str,
) -> Result<Id> {
    let owner = Id::new_v4();
    let stage = store.stage_begin(owner, StagingPurpose::Image)?;
    fs::write(
        &stage.path,
        format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='6'><rect width='8' height='6' fill='{color}'/></svg>"
        ),
    )?;
    let Staged::Image { id, asset } = store.stage_commit(owner, stage.token, state, renderer)?
    else {
        panic!("Expected image");
    };
    state.assets.insert(id, asset);
    Ok(id)
}

fn import_file(
    store: &mut Store,
    state: &AppState,
    renderer: &mut Renderer,
    path: &Path,
    replace: bool,
) -> Result<AppState> {
    let owner = Id::new_v4();
    let stage = store.stage_begin(owner, StagingPurpose::Import { replace })?;
    fs::copy(path, &stage.path)?;
    let Staged::Import { candidate, .. } =
        store.stage_commit(owner, stage.token, state, renderer)?
    else {
        panic!("Expected import");
    };
    Ok(candidate)
}

#[test]
fn existing_assets_or_backup_prevent_missing_settings_defaults() -> Result<()> {
    for evidence in ["asset", "backup", "marker"] {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        match evidence {
            "asset" => fs::write(store.asset_path(Id::new_v4()), b"preserved")?,
            "backup" => fs::write(directory.path().join("state.backup.json"), b"preserved")?,
            _ => fs::write(directory.path().join("initialized"), b"1")?,
        }
        assert!(store.load().is_err());
        assert!(!directory.path().join("state.json").exists());
    }
    Ok(())
}

#[test]
fn staging_is_owned_bounded_and_removed_on_disconnect_or_failure() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState::default();
    let owner = Id::new_v4();
    let stage = store.stage_begin(owner, StagingPurpose::Image)?;
    assert!(store.discard_staging(Id::new_v4(), stage.token).is_err());
    assert!(
        store
            .stage_commit(Id::new_v4(), stage.token, &state, &mut renderer)
            .is_err()
    );
    assert!(stage.path.exists());
    fs::write(&stage.path, b"invalid image")?;
    assert!(
        store
            .stage_commit(owner, stage.token, &state, &mut renderer)
            .is_err()
    );
    assert!(!stage.path.exists());
    for _ in 0..4 {
        store.stage_begin(owner, StagingPurpose::Image)?;
    }
    assert!(store.stage_begin(owner, StagingPurpose::Image).is_err());
    assert_eq!(store.disconnect(owner)?, 4);
    assert_eq!(fs::read_dir(directory.path().join("staging"))?.count(), 0);
    let stage = store.stage_begin(owner, StagingPurpose::Import { replace: false })?;
    fs::OpenOptions::new()
        .write(true)
        .open(&stage.path)?
        .set_len(MAX_PORTABLE_BYTES + 1)?;
    assert!(
        store
            .stage_commit(owner, stage.token, &state, &mut renderer)
            .is_err()
    );
    assert!(!stage.path.exists());
    Ok(())
}

#[test]
fn image_content_is_deduplicated_verified_and_never_overwritten() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let mut state = AppState::default();
    let id = add_image(&mut store, &mut state, &mut renderer, "red")?;
    assert_eq!(id, add_image(&mut store, &mut state, &mut renderer, "red")?);
    assert_eq!(state.assets.len(), 1);
    let data = store.read_asset(&state, id)?;
    assert!(store.adopt(id, &data).is_err());
    assert_eq!(fs::read(store.asset_path(id))?, data.bytes.as_ref());
    store.save(&state)?;
    fs::write(store.asset_path(id), b"tampered")?;
    assert!(store.read_asset(&state, id).is_err());
    assert!(store.load().is_err());
    Ok(())
}

#[test]
fn all_data_round_trip_remaps_every_reference_and_backup_is_self_contained() -> Result<()> {
    let source_dir = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let mut source = Store::open(source_dir.path())?;
    let mut target = Store::open(target_dir.path())?;
    let mut renderer = Renderer::new()?;
    let mut state = AppState::default();
    let image = add_image(&mut source, &mut state, &mut renderer, "red")?;
    state.presets[0].layers[0].content = Content::Image(Image::new(image, 8.0, 6.0));
    let old_preset = state.presets[0].id;
    let old_profile = state.profiles[0].id;
    state.profiles[0].bindings.push(Binding::new(
        InputChord {
            trigger: InputKey::Mouse { button: 4 },
            modifiers: Vec::new(),
        },
        BindingAction::ShowPresetWhileHeld { preset: old_preset },
    ));
    state.profiles[0].positions.push(SavedPosition {
        id: Id::new_v4(),
        name: "Position".into(),
        offset: Point { x: 10.0, y: 0.0 },
        scale: 1.0,
    });
    let path = exports.path().join("all.json");
    source.export(&state, ExportKind::All, &path, &mut renderer)?;
    let current = AppState::default();
    let restored = import_file(&mut target, &current, &mut renderer, &path, true)?;
    assert_eq!(restored.presets.len(), state.presets.len());
    assert_ne!(restored.presets[0].id, old_preset);
    assert_ne!(restored.profiles[0].id, old_profile);
    assert_eq!(restored.active_profile, restored.profiles[0].id);
    assert_eq!(restored.profiles[0].selected_preset, restored.presets[0].id);
    assert_eq!(
        restored.profiles[0].bindings[0].action.target(),
        Some(restored.presets[0].id)
    );
    assert_ne!(
        restored.profiles[0].positions[0].id,
        state.profiles[0].positions[0].id
    );
    let restored_image = restored.presets[0].layers[0].content.asset_id().unwrap();
    assert_ne!(restored_image, image);
    assert_eq!(
        target.read_asset(&restored, restored_image)?.bytes,
        source.read_asset(&state, image)?.bytes
    );
    target.save(&restored)?;
    assert_eq!(target.load()?, restored);
    assert_eq!(fs::read_dir(target_dir.path().join("backups"))?.count(), 1);
    let merged = import_file(&mut target, &restored, &mut renderer, &path, false)?;
    assert_eq!(merged.presets.len(), restored.presets.len() * 2);
    assert_eq!(merged.assets.len(), 1);
    assert_eq!(merged.active_profile, restored.active_profile);
    Ok(())
}

#[test]
fn preset_and_profile_exports_include_only_their_dependencies() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState::default();
    let preset = exports.path().join("preset.json");
    store.export(
        &state,
        ExportKind::Preset {
            preset: state.presets[0].id,
        },
        &preset,
        &mut renderer,
    )?;
    let merged = import_file(&mut store, &state, &mut renderer, &preset, false)?;
    assert_eq!(merged.presets.len(), state.presets.len() + 1);
    assert_eq!(merged.profiles.len(), state.profiles.len());
    assert!(import_file(&mut store, &state, &mut renderer, &preset, true).is_err());
    let profile = exports.path().join("profile.json");
    store.export(
        &state,
        ExportKind::Profile {
            profile: state.profiles[0].id,
        },
        &profile,
        &mut renderer,
    )?;
    let merged = import_file(&mut store, &state, &mut renderer, &profile, false)?;
    assert_eq!(merged.presets.len(), state.presets.len() + 1);
    assert_eq!(merged.profiles.len(), state.profiles.len() + 1);
    let png = exports.path().join("crosshair.png");
    store.export(
        &state,
        ExportKind::Png {
            preset: state.presets[0].id,
        },
        &png,
        &mut renderer,
    )?;
    assert!(fs::read(png)?.starts_with(b"\x89PNG"));
    assert!(
        store
            .export(
                &state,
                ExportKind::All,
                &directory.path().join("state.json"),
                &mut renderer
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn failed_adoption_rolls_back_sources_and_malformed_import_preserves_live_data() -> Result<()> {
    let source_dir = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let mut source = Store::open(source_dir.path())?;
    let mut target = Store::open(target_dir.path())?;
    let mut renderer = Renderer::new()?;
    let mut state = AppState::default();
    add_image(&mut source, &mut state, &mut renderer, "red")?;
    add_image(&mut source, &mut state, &mut renderer, "green")?;
    let path = exports.path().join("all.json");
    source.export(&state, ExportKind::All, &path, &mut renderer)?;
    let current = AppState::default();
    target.save(&current)?;
    target.fail_adoption_after.set(Some(1));
    assert!(import_file(&mut target, &current, &mut renderer, &path, false).is_err());
    assert_eq!(fs::read_dir(target_dir.path().join("assets"))?.count(), 0);
    assert_eq!(fs::read_dir(target_dir.path().join("staging"))?.count(), 0);
    assert_eq!(target.load()?, current);
    target.fail_adoption_after.set(None);
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    json["state"]["profiles"][0]["selected_preset"] = serde_json::json!(Id::new_v4());
    fs::write(&path, serde_json::to_vec(&json)?)?;
    assert!(import_file(&mut target, &current, &mut renderer, &path, false).is_err());
    assert_eq!(target.load()?, current);
    assert_eq!(fs::read_dir(target_dir.path().join("assets"))?.count(), 0);
    Ok(())
}

#[test]
fn backup_recovery_preserves_corruption_and_gc_respects_backup_and_history() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let mut state = AppState::default();
    let image = add_image(&mut store, &mut state, &mut renderer, "red")?;
    store.save(&state)?;
    state.assets.clear();
    state.revision += 1;
    store.save(&state)?;
    assert_eq!(store.collect_assets(&state, &BTreeSet::new())?, 0);
    store.save(&state)?;
    assert_eq!(store.collect_assets(&state, &BTreeSet::from([image]))?, 0);
    assert_eq!(store.collect_assets(&state, &BTreeSet::new())?, 1);
    fs::write(directory.path().join("state.json"), b"{broken")?;
    assert!(store.save(&state).is_err());
    assert_eq!(store.recover_backup()?, state);
    assert_eq!(store.load()?, state);
    assert!(
        fs::read_dir(directory.path())?.any(|entry| entry.is_ok_and(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("state-recovery-")))
    );
    assert!(store.collect_assets(&state, &BTreeSet::new()).is_err());
    Ok(())
}

#[test]
fn interrupted_atomic_write_retains_destination_and_removes_temporary_file() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("export.json");
    fs::write(&path, b"original")?;
    let result = super::state::atomic_write(&path, |file| {
        file.write_all(b"partial")?;
        anyhow::bail!("Injected disk failure")
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path)?, b"original");
    assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[test]
fn staged_document_rejects_wrong_target_or_revision_without_replacing_state() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState::default();
    let owner = Id::new_v4();
    let stage = store.stage_begin(
        owner,
        StagingPurpose::Document {
            preset: state.presets[0].id,
            expected_revision: 1,
        },
    )?;
    fs::write(&stage.path, serde_json::to_vec(&state.presets[0])?)?;
    assert!(
        store
            .stage_commit(owner, stage.token, &state, &mut renderer)
            .is_err()
    );
    let stage = store.stage_begin(
        owner,
        StagingPurpose::Document {
            preset: state.presets[0].id,
            expected_revision: 0,
        },
    )?;
    fs::write(&stage.path, serde_json::to_vec(&state.presets[1])?)?;
    assert!(
        store
            .stage_commit(owner, stage.token, &state, &mut renderer)
            .is_err()
    );
    Ok(())
}

#[test]
fn explicit_reset_and_recovery_export_preserve_unreadable_originals() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState {
        revision: 42,
        ..Default::default()
    };
    fs::write(directory.path().join("state.json"), b"{damaged-original")?;
    let path = exports.path().join("recovery.json");
    store.export(&state, ExportKind::Recovery, &path, &mut renderer)?;
    let exported: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    assert_eq!(exported["format"], "crosshair-rs-recovery");
    assert_eq!(exported["files"][0]["path"], "state.json");
    let reset = store.reset(&state)?;
    assert_eq!(reset.revision, 43);
    assert_eq!(store.load()?, reset);
    let preserved = fs::read_dir(directory.path())?
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_name().to_string_lossy().starts_with("recovery-"))
        .unwrap();
    assert_eq!(
        fs::read(preserved.path().join("state.json"))?,
        b"{damaged-original"
    );
    assert_eq!(
        serde_json::from_slice::<AppState>(&fs::read(preserved.path().join("ram-state.json"))?)?,
        state
    );
    Ok(())
}

#[test]
fn portable_component_limits_apply_before_unbounded_string_allocation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState::default();
    let owner = Id::new_v4();
    let stage = store.stage_begin(owner, StagingPurpose::Import { replace: false })?;
    let mut file = fs::File::create(&stage.path)?;
    file.write_all(b"{\"format\":\"")?;
    let chunk = [b'x'; 64 * 1024];
    for _ in 0..130 {
        file.write_all(&chunk)?;
    }
    file.write_all(b"\"}")?;
    drop(file);
    assert!(
        store
            .stage_commit(owner, stage.token, &state, &mut renderer)
            .is_err()
    );
    assert!(!stage.path.exists());
    Ok(())
}

#[test]
fn exporting_unsent_draft_never_mutates_authoritative_state() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let mut renderer = Renderer::new()?;
    let state = AppState::default();
    store.save(&state)?;
    let mut draft = state.presets[0].clone();
    draft.name = "Unsent draft".into();
    for png in [false, true] {
        let destination = exports
            .path()
            .join(if png { "draft.png" } else { "draft.json" });
        let owner = Id::new_v4();
        let stage = store.stage_begin(
            owner,
            StagingPurpose::ExportDraft {
                destination: destination.to_string_lossy().into_owned(),
                png,
            },
        )?;
        fs::write(&stage.path, serde_json::to_vec(&draft)?)?;
        assert!(matches!(
            store.stage_commit(owner, stage.token, &state, &mut renderer)?,
            Staged::Exported
        ));
        assert!(destination.exists());
        assert_eq!(store.load()?, state);
        if !png {
            let json: serde_json::Value = serde_json::from_slice(&fs::read(destination)?)?;
            assert_eq!(json["state"]["presets"][0]["name"], "Unsent draft");
        }
    }
    Ok(())
}

#[test]
fn save_queued_before_asset_publication_cannot_collect_new_adoptions() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(directory.path())?;
    let state = AppState::default();
    let mut renderer = Renderer::new()?;
    let owner = Id::new_v4();
    store.save(&state)?;
    let stage = store.stage_begin(owner, StagingPurpose::Image)?;
    fs::write(&stage.path, b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4'/></svg>")?;
    let Staged::Image { id, asset } =
        store.stage_commit(owner, stage.token, &state, &mut renderer)?
    else {
        panic!("Expected image");
    };
    store.save(&state)?;
    assert_eq!(store.collect_assets(&state, &BTreeSet::new())?, 0);
    assert!(store.asset_path(id).exists());
    let mut published = state.clone();
    published.assets.insert(id, asset);
    store.save(&published)?;
    assert!(store.pending_assets.borrow().is_empty());
    store.disconnect(owner)?;
    assert_eq!(store.collect_assets(&published, &BTreeSet::new())?, 0);
    Ok(())
}

/// Run explicitly: this fixture uses about 1.5 GiB of temporary disk, while each
/// decoder sees only one bounded asset at a time.
#[test]
#[ignore = "near-512 MiB streaming backup/restore stress fixture"]
fn near_limit_portable_backup_round_trip() -> Result<()> {
    use std::sync::Arc;
    let source_dir = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let exports = tempfile::tempdir()?;
    let source = Store::open(source_dir.path())?;
    let mut target = Store::open(target_dir.path())?;
    let mut state = AppState::default();
    let mut renderer = Renderer::new()?;
    for index in 0..48 {
        let size =
            crosshair_core::MAX_ASSET_BYTES as usize - if index == 47 { 64 * 1024 } else { 0 };
        let mut source_bytes = format!("<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8' fill='red'/><!--fixture-{index} ").into_bytes();
        let suffix = b"--></svg>";
        source_bytes.resize(size - suffix.len(), b' ');
        source_bytes.extend_from_slice(suffix);
        let (data, metadata) = super::assets::inspect(Arc::from(source_bytes), &mut renderer)?;
        let id = Id::new_v4();
        source.adopt(id, &data)?;
        state.assets.insert(id, metadata);
    }
    crosshair_core::validate_state(&state)?;
    let path = exports.path().join("near-limit.json");
    source.export(&state, ExportKind::All, &path, &mut renderer)?;
    let bytes = fs::metadata(&path)?.len();
    assert!(bytes > 511 * 1024 * 1024 && bytes <= MAX_PORTABLE_BYTES);
    let restored = import_file(
        &mut target,
        &AppState::default(),
        &mut renderer,
        &path,
        true,
    )?;
    assert_eq!(restored.assets.len(), 48);
    target.save(&restored)?;
    assert_eq!(target.load()?, restored);
    Ok(())
}
