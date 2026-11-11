mod assets;
mod portable;
mod recovery;
mod staging;
mod state;

pub use assets::read_asset_at;
pub use staging::{Stage, Staged};
pub use state::{Store, default_data_directory};

/// Apply only when clipboard/import leases have ended, and normalize retained
/// history at the same boundary so cleanup does not invalidate undo comparisons.
pub fn prune_asset_index(
    state: &mut crosshair_core::AppState,
    pending: &std::collections::BTreeSet<crosshair_core::Id>,
) -> bool {
    let referenced: std::collections::BTreeSet<_> = state
        .presets
        .iter()
        .flat_map(|preset| &preset.layers)
        .filter_map(|layer| layer.content.asset_id())
        .chain(pending.iter().copied())
        .collect();
    let previous = state.assets.len();
    state.assets.retain(|id, _| referenced.contains(id));
    state.assets.len() != previous
}

#[cfg(test)]
mod tests;
