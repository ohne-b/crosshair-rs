mod desktop;
mod monitors;
pub mod render;
pub mod widget;

pub use desktop::DesktopOverlay;
pub use monitors::{Monitor, enumerate_monitors};

/// Match physical pixel placement for odd and even extents on every output.
pub fn centered_origin(center: [i32; 2], size: [u32; 2]) -> [i32; 2] {
    [
        center[0] - (size[0] / 2) as i32,
        center[1] - (size[1] / 2) as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_and_even_bounds_preserve_one_pixel_alignment() {
        assert_eq!(centered_origin([960, 540], [32, 32]), [944, 524]);
        assert_eq!(centered_origin([960, 540], [33, 33]), [944, 524]);
        assert_eq!(centered_origin([-960, 540], [33, 33]), [-976, 524]);
    }
}
