//! Bounded wire contracts; transport authentication belongs to the native hosts.
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Binding, Id, InputChord, PROTOCOL_VERSION, Preset, Profile, Settings};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const CHUNK_BYTES: usize = 64 * 1024;
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub protocol: u32,
    pub version: String,
    pub command: Command,
}

impl Request {
    pub fn new(id: u64, command: Command) -> Self {
        Self {
            id,
            protocol: PROTOCOL_VERSION,
            version: PRODUCT_VERSION.into(),
            command,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Subscribe,
    Snapshot,
    ReadSnapshot {
        token: Id,
        offset: u64,
    },
    GetPreset {
        preset: Id,
    },
    SelectPreset {
        preset: Id,
    },
    ToggleVisibility,
    SetVisibility {
        visible: bool,
    },
    BeginEdit {
        preset: Id,
        expected_revision: u64,
    },
    ReplacePreset {
        preset: Preset,
        expected_revision: u64,
    },
    PreviewPreset {
        preset: Preset,
        expected_revision: u64,
    },
    ClearPreview,
    EndEdit,
    CancelEdit,
    Undo,
    Redo,
    NewPreset {
        name: String,
    },
    AddPreset {
        preset: Preset,
    },
    DuplicatePreset {
        preset: Id,
    },
    DeletePreset {
        preset: Id,
        replacement: Id,
    },
    ReorderPresets {
        order: Vec<Id>,
    },
    SetFavorite {
        preset: Id,
        favorite: bool,
    },
    UpsertProfile {
        profile: Profile,
        expected_revision: u64,
    },
    SetBindings {
        profiles: Vec<BindingSet>,
        global_toggle: Option<InputChord>,
        expected_revision: u64,
    },
    SelectProfile {
        profile: Id,
    },
    DeleteProfile {
        profile: Id,
        replacement: Id,
    },
    SetSettings {
        settings: Settings,
        expected_revision: u64,
    },
    BeginCapture {
        token: Id,
    },
    CancelCapture {
        token: Id,
    },
    BeginStaging {
        purpose: StagingPurpose,
    },
    CommitStaging {
        token: Id,
    },
    DiscardStaging {
        token: Id,
    },
    ReadAsset {
        asset: Id,
        offset: u64,
    },
    Export {
        kind: ExportKind,
        destination: String,
    },
    Flush,
    RecoverBackup,
    ResetSettings,
    OpenDataDirectory,
    OpenGameBar,
    CheckUpdate,
    DownloadUpdate,
    CancelUpdate,
    InstallUpdate,
    OpenEditor,
    ConfirmQuit,
    CancelQuit,
    Quit {
        discard_unsaved: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingSet {
    pub profile: Id,
    pub bindings: Vec<Binding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StagingPurpose {
    Image,
    Document {
        preset: Id,
        expected_revision: u64,
        preview: bool,
    },
    Import {
        replace: bool,
    },
    ExportDraft {
        destination: String,
        format: DraftFormat,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftFormat {
    Preset,
    Png,
    Svg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportKind {
    Preset { preset: Id },
    Profile { profile: Id },
    All,
    Png { preset: Id },
    Svg { preset: Id },
    Recovery,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Ack {
        document_revision: Option<u64>,
    },
    Snapshot {
        token: Id,
        revision: u64,
        bytes: u64,
    },
    Chunk {
        bytes: Vec<u8>,
        complete: bool,
    },
    Preset {
        preset: Preset,
    },
    Created {
        id: Id,
    },
    Staging {
        token: Id,
        path: String,
        max_bytes: u64,
    },
    Asset {
        id: Id,
        width: u32,
        height: u32,
    },
    Status {
        status: EngineStatus,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub id: u64,
    pub revision: u64,
    pub result: Result<Reply, ServiceError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Response(Response),
    Changed {
        revision: u64,
        document: Option<Id>,
        structure: bool,
        status: EngineStatus,
    },
    Captured {
        token: Id,
        chord: Option<InputChord>,
        cancelled: bool,
    },
    Update {
        status: UpdateStatus,
    },
    FocusEditor,
    PrepareClose {
        updating: bool,
    },
    CloseCancelled,
    Closing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Invalid,
    Conflict,
    NotFound,
    Storage,
    Unavailable,
    Permission,
    VersionMismatch,
    Busy,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineStatus {
    pub revision: u64,
    pub active_profile: Option<Id>,
    pub selected_preset: Option<Id>,
    pub visible: bool,
    pub monitors: Vec<DisplayInfo>,
    pub widget: WidgetStatus,
    pub unsaved: bool,
    pub recovery_required: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub save_error: Option<String>,
    pub render_error: Option<String>,
    pub update: UpdateStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub bounds: [i32; 4],
    pub primary: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetStatus {
    pub installed: bool,
    pub connected: bool,
    pub pinned: bool,
    pub visible: bool,
    pub click_through: bool,
    pub positioned: bool,
    pub dpi: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Installing,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateStatus {
    pub revision: u64,
    pub phase: UpdatePhase,
    pub current_version: String,
    pub version: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub restart_required: bool,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: UpdatePhase::Idle,
            current_version: PRODUCT_VERSION.into(),
            version: None,
            downloaded: 0,
            total: None,
            error: None,
            restart_required: false,
        }
    }
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Control message exceeds the limit",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid control message length",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_rejects_oversize_before_reading_payload() {
        let mut header = ((MAX_FRAME_BYTES + 1) as u32)
            .to_le_bytes()
            .as_slice()
            .to_owned();
        assert_eq!(
            read_frame::<Request>(&mut header.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        header = vec![0, 0, 0, 0];
        assert!(read_frame::<Request>(&mut header.as_slice()).is_err());
    }

    #[test]
    fn round_trip_and_truncation() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Request::new(17, Command::ToggleVisibility)).unwrap();
        let parsed: Request = read_frame(&mut bytes.as_slice()).unwrap();
        assert_eq!(parsed.id, 17);
        assert!(matches!(parsed.command, Command::ToggleVisibility));
        bytes.pop();
        assert_eq!(
            read_frame::<Request>(&mut bytes.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
