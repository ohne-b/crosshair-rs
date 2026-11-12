use std::fs;

use anyhow::Result;
use crosshair_core::{protocol::Command, *};

use crate::{app::App, storage::Store};

fn application() -> Result<(tempfile::TempDir, Store, App)> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let app = App::open(&store, true)?;
    Ok((directory, store, app))
}

#[test]
fn capture_cancel_is_idempotent_but_cannot_cancel_another_owner() -> Result<()> {
    let (_directory, _store, mut app) = application()?;
    let owner = Id::new_v4();
    app.command(owner, Command::BeginCapture { token: Id::nil() })?;
    assert!(
        app.command(Id::new_v4(), Command::CancelCapture { token: Id::nil() })
            .is_err()
    );
    assert!(app.input.capturing());
    app.command(owner, Command::CancelCapture { token: Id::nil() })?;
    app.command(owner, Command::CancelCapture { token: Id::nil() })?;
    assert!(!app.input.capturing());
    Ok(())
}

#[test]
fn pinned_edit_and_undo_preserve_global_selection() -> Result<()> {
    let (_directory, store, mut app) = application()?;
    let owner = Id::new_v4();
    let mut edited = app.state.presets[0].clone();
    let selected = app.state.presets[1].id;
    app.command(
        owner,
        Command::BeginEdit {
            preset: edited.id,
            expected_revision: edited.revision,
        },
    )?;
    app.command(owner, Command::SelectPreset { preset: selected })?;
    edited.name = "Pinned document".into();
    app.command(
        owner,
        Command::ReplacePreset {
            preset: edited.clone(),
            expected_revision: edited.revision,
        },
    )?;
    app.command(owner, Command::EndEdit)?;
    assert_eq!(app.state.selected_preset().unwrap().id, selected);
    assert_eq!(app.state.preset(edited.id).unwrap().name, "Pinned document");
    app.command(owner, Command::Undo)?;
    assert_eq!(app.state.selected_preset().unwrap().id, selected);
    assert_eq!(app.state.preset(edited.id).unwrap().name, "Classic");
    store.save(&app.state)?;
    app.saved(app.state.revision);
    assert_eq!(store.load()?, app.state);
    Ok(())
}

#[test]
fn inline_rename_is_atomic_and_undoable_without_changing_the_active_preset() -> Result<()> {
    let (_directory, _store, mut app) = application()?;
    let owner = Id::new_v4();
    let active = app.state.selected_preset().unwrap().id;
    let original = app.state.presets[1].clone();
    let mut renamed = original.clone();
    renamed.name = "Small dot".into();
    app.command(
        owner,
        Command::ReplacePreset {
            preset: renamed,
            expected_revision: original.revision,
        },
    )?;
    let stored = app.state.preset(original.id).unwrap();
    assert_eq!(stored.name, "Small dot");
    assert_eq!(stored.layers, original.layers);
    assert_eq!(app.state.selected_preset().unwrap().id, active);
    let before_invalid = app.state.clone();
    let mut invalid = stored.clone();
    invalid.name = " ".into();
    assert!(
        app.command(
            owner,
            Command::ReplacePreset {
                expected_revision: invalid.revision,
                preset: invalid,
            }
        )
        .is_err()
    );
    assert_eq!(app.state, before_invalid);
    app.command(owner, Command::Undo)?;
    assert_eq!(app.state.preset(original.id).unwrap().name, original.name);
    assert_eq!(app.state.selected_preset().unwrap().id, active);
    Ok(())
}

#[test]
fn invalid_locked_stale_and_oversized_edits_leave_state_unchanged() -> Result<()> {
    let (_directory, store, mut app) = application()?;
    let owner = Id::new_v4();
    let mut preset = app.state.presets[0].clone();
    preset.layers[0].locked = true;
    app.command(
        owner,
        Command::ReplacePreset {
            preset: preset.clone(),
            expected_revision: 0,
        },
    )?;
    preset = app.state.presets[0].clone();
    let before = app.state.clone();
    preset.layers[0].transform.x = 10.0;
    assert!(
        app.command(
            owner,
            Command::ReplacePreset {
                preset: preset.clone(),
                expected_revision: preset.revision
            }
        )
        .is_err()
    );
    assert_eq!(app.state, before);
    assert!(
        app.command(
            owner,
            Command::ReplacePreset {
                preset,
                expected_revision: 0
            }
        )
        .is_err()
    );
    assert_eq!(app.state, before);
    let mut other = app.state.presets[1].clone();
    other.layers[0].effects.blur = 64.0;
    other.layers[0].transform.scale = 16.0;
    other.layers[0].content = Content::Ellipse(Shape {
        width: 1000.0,
        height: 1000.0,
        ..Default::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut worker = crate::work::Work::start(store, move |result| {
        let _ = sender.send(result);
    })?;
    worker.try_submit(
        1,
        crate::work::Job::ValidatePreset {
            state: Box::new(app.state.clone()),
            preset: Box::new(other),
        },
    )?;
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))?
            .result
            .is_err()
    );
    worker.try_submit(2, crate::work::Job::Shutdown)?;
    assert!(matches!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))?
            .result?,
        crate::work::WorkReply::Stopped
    ));
    worker.join()?;
    assert_eq!(app.state, before);
    Ok(())
}

#[test]
fn deleting_referenced_preset_remaps_once_and_undo_restores_references() -> Result<()> {
    let (_directory, _store, mut app) = application()?;
    let owner = Id::new_v4();
    let target = app.state.presets[0].id;
    let replacement = app.state.presets[1].id;
    app.state.profiles[0].bindings.push(Binding::new(
        InputChord::key(0x12),
        BindingAction::SelectPreset { preset: target },
    ));
    let before = app.state.clone();
    app.command(
        owner,
        Command::DeletePreset {
            preset: target,
            replacement,
        },
    )?;
    assert!(app.state.preset(target).is_none());
    assert_eq!(app.state.profiles[0].selected_preset, replacement);
    assert_eq!(
        app.state.profiles[0].bindings[0].action.target(),
        Some(replacement)
    );
    app.command(owner, Command::Undo)?;
    assert_eq!(app.state.profiles[0].bindings, before.profiles[0].bindings);
    assert!(app.state.preset(target).is_some());
    Ok(())
}

#[test]
fn recovery_refuses_mutations_until_an_explicit_atomic_reset() -> Result<()> {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("state.json"), b"{broken settings")?;
    let store = Store::open(directory.path())?;
    let mut app = App::open(&store, true)?;
    assert!(app.status.recovery_required);
    assert!(!app.status().visible);
    assert!(app.command(Id::nil(), Command::ToggleVisibility).is_err());
    assert_eq!(
        fs::read(directory.path().join("state.json"))?,
        b"{broken settings"
    );
    app.state = store.reset(&app.state)?;
    app.status.recovery_required = false;
    assert!(!app.status.recovery_required);
    assert_eq!(store.load()?, app.state);
    assert!(fs::read_dir(directory.path())?.any(|entry| {
        entry.is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with("recovery-"))
    }));
    Ok(())
}

#[test]
fn failed_save_retains_memory_and_does_not_create_an_idle_retry_loop() -> Result<()> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let (directory, store, mut app) = application()?;
    store.save(&app.state)?;
    app.saved(app.state.revision);
    let original = store.load()?;
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(directory.path().join("state.json"))?;
    app.command(Id::nil(), Command::ToggleVisibility)?;
    app.dirty();
    let error = store.save(&app.state).unwrap_err();
    app.save_failed(error.to_string());
    app.changed(None, false);
    assert!(app.status.unsaved);
    assert!(app.save_at.is_none());
    assert_ne!(app.state.base_visible, original.base_visible);
    drop(locked);
    assert_eq!(store.load()?, original);
    store.save(&app.state)?;
    app.saved(app.state.revision);
    assert!(!app.status.unsaved);
    assert_eq!(store.load()?, app.state);
    Ok(())
}

#[test]
fn selection_cancels_capture_and_allows_a_new_capture() -> Result<()> {
    let (_directory, _store, mut app) = application()?;
    let owner = Id::new_v4();
    app.command(owner, Command::BeginCapture { token: Id::nil() })?;
    app.command(
        owner,
        Command::SelectPreset {
            preset: app.state.presets[1].id,
        },
    )?;
    assert!(!app.input.capturing());
    app.command(owner, Command::BeginCapture { token: Id::nil() })?;
    assert!(app.input.capturing());
    Ok(())
}

#[test]
fn stale_cancel_does_not_stop_a_replacement_capture() -> Result<()> {
    let (_directory, _store, mut app) = application()?;
    let owner = Id::new_v4();
    let old = Id::new_v4();
    let current = Id::new_v4();
    app.command(owner, Command::BeginCapture { token: old })?;
    app.command(owner, Command::CancelCapture { token: old })?;
    app.command(owner, Command::BeginCapture { token: current })?;
    app.command(owner, Command::CancelCapture { token: old })?;
    assert!(app.input.capturing());
    assert_eq!(app.capture_owner, Some((owner, current)));
    app.command(owner, Command::CancelCapture { token: current })?;
    assert!(!app.input.capturing());
    Ok(())
}
