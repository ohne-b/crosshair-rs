use crate::{Error, Result};

const MIB: usize = 1024 * 1024;

/// Gallery admission limits, deliberately tighter than the app's backup import.
/// Callers may LOWER these values, never raise them above the default hard caps.
/// These are input/work budgets, not a hard process-memory or wall-time sandbox.
#[derive(Clone, Copy, Debug)]
pub struct ValidationLimits {
    pub max_json_bytes: usize,
    pub max_json_nodes: usize,
    pub max_json_depth: usize,
    pub max_assets: usize,
    pub max_asset_bytes: usize,
    pub max_total_asset_bytes: usize,
    pub max_image_side: u32,
    pub max_image_pixels: u64,
    pub max_total_image_pixels: u64,
    pub max_layers: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_json_bytes: 16 * MIB,
            max_json_nodes: 100_000,
            max_json_depth: 64,
            max_assets: 64,
            max_asset_bytes: 8 * MIB,
            max_total_asset_bytes: 12 * MIB,
            max_image_side: 4096,
            max_image_pixels: 16 * MIB as u64,
            max_total_image_pixels: 32 * MIB as u64,
            max_layers: 64,
        }
    }
}

impl ValidationLimits {
    pub(crate) fn check(self) -> Result<Self> {
        let hard = Self::default();
        macro_rules! bounded {
            ($($field:ident),+ $(,)?) => {$(
                if self.$field == 0 || self.$field > hard.$field {
                    return Err(Error::InvalidLimits(concat!(
                        stringify!($field), " must be positive and no greater than the default"
                    )));
                }
            )+};
        }
        bounded!(
            max_json_bytes,
            max_json_nodes,
            max_json_depth,
            max_assets,
            max_asset_bytes,
            max_total_asset_bytes,
            max_image_side,
            max_image_pixels,
            max_total_image_pixels,
            max_layers
        );
        Ok(self)
    }
}
