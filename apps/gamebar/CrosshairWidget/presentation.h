#pragma once
#include "wire.h"
#include <algorithm>
#include <cmath>

namespace crosshair {
// Guard with the mailbox mutex. A received Off/new connection supersedes work
// already awaiting a Game Bar resize, just as it supersedes a queued bitmap.
struct LatestFrame {
    std::uint64_t connection{}, generation{};
    bool observe(wire::Frame const& frame) {
        if (frame.connection < connection ||
            (frame.connection == connection && frame.header.generation <= generation)) return false;
        connection = frame.connection; generation = frame.header.generation;
        return true;
    }
    bool matches(wire::Frame const& frame) const {
        return matches(frame.connection, frame.header.generation);
    }
    bool matches(std::uint64_t epoch, std::uint64_t value) const {
        return epoch == connection && value == generation;
    }
};

struct BitmapState {
    bool ready{}, shown{};
    void clear() { ready = shown = false; }
    void position(wire::Header const& header, bool fitted) {
        ready = fitted && (header.flags & wire::visible) && !(header.flags & wire::close);
        shown = ready && !(header.flags & wire::prepare);
    }
    std::uint32_t flags(bool latest, bool shell_visible, bool opaque) const {
        if (!latest || !ready) return 0;
        return wire::prepared | (shown && shell_visible && opaque ? wire::presented : 0u);
    }
};

struct PhysicalBounds { std::int64_t left{}, top{}, right{}, bottom{}; };
inline bool fits(wire::Header const& h, PhysicalBounds bounds) {
    auto left = std::int64_t(h.center_x) - h.width / 2;
    auto top = std::int64_t(h.center_y) - h.height / 2;
    return left >= bounds.left && top >= bounds.top &&
        left + h.width <= bounds.right && top + h.height <= bounds.bottom;
}
struct DesiredSize { float width{}, height{}; };
inline DesiredSize desired_size(wire::Header const& h, PhysicalBounds bounds, double scale) {
    // Resizing may preserve the widget center or its origin. Request enough room
    // for either, then verify actual physical bounds; never shift the crosshair.
    auto extent = [scale](std::int32_t center, std::uint32_t pixels, std::int64_t first, std::int64_t last, float minimum) {
        double centered = 2 * std::abs(double(center) - (first + last) / 2.0) + pixels;
        double anchored = double(center) + pixels / 2.0 - first;
        return std::clamp(float(std::max({double(pixels), centered, anchored}) / scale + 16), minimum, 4096.0f);
    };
    return {extent(h.center_x, h.width, bounds.left, bounds.right, 200),
            extent(h.center_y, h.height, bounds.top, bounds.bottom, 100)};
}
}
