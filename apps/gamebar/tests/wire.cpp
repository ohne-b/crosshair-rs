#include "../CrosshairWidget/wire.h"
#include "../CrosshairWidget/presentation.h"
#include <cassert>
#include <iostream>
#include <limits>

using namespace crosshair::wire;
int main() {
    std::array<std::byte, header_size> bytes{};
    write<std::uint32_t>(bytes, 0, 0x46535243);
    write<std::uint16_t>(bytes, 4, version); write<std::uint16_t>(bytes, 6, header_size);
    write<std::uint64_t>(bytes, 8, 0x0102030405060708);
    write<std::uint32_t>(bytes, 16, 1024); write<std::uint32_t>(bytes, 20, 1024);
    write<std::uint32_t>(bytes, 24, 4096); write<std::int32_t>(bytes, 28, -1920);
    write<std::int32_t>(bytes, 32, 1080); write<std::uint32_t>(bytes, 36, visible | center);
    auto parsed = decode(bytes);
    assert(parsed.byte_count() == 4 * 1024 * 1024 && parsed.center_x == -1920);
    assert(parsed.generation == 0x0102030405060708);
    auto reject = [](auto const& value) { try { (void)decode(value); } catch (std::runtime_error const&) { return; } assert(false); };
    auto bad = bytes; bad[0] = std::byte{}; reject(bad);
    bad = bytes; write<std::uint16_t>(bad, 4, 2); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 16, 1025); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 20, std::numeric_limits<std::uint32_t>::max()); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 24, 4095); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, 16); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, prepare); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, visible | prepare | close); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, visible | close); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, 0); reject(bad);
    reject(std::span(bytes).first(39));
    write<std::uint32_t>(bad, 16, 0); write<std::uint32_t>(bad, 20, 0); write<std::uint32_t>(bad, 24, 0);
    assert(decode(bad).byte_count() == 0);
    auto empty = bad;
    write<std::uint32_t>(bad, 36, prepare); reject(bad);
    bad = empty; write<std::uint32_t>(bad, 36, center); reject(bad);
    bad = empty; write<std::uint32_t>(bad, 36, close | center); reject(bad);
    bad = empty; write<std::uint32_t>(bad, 36, close | prepare); reject(bad);
    bad = bytes; write<std::uint32_t>(bad, 36, visible | prepare | center);
    auto preparing = decode(bad);
    assert(preparing.byte_count() == parsed.byte_count() && (preparing.flags & prepare));
    Status status{42, 127, 144, -1920, 0, 1920, 1080, -960, 540, 1234, 2, 1ull << 32};
    auto encoded = encode(status);
    assert(read<std::uint32_t>(encoded, 0) == 0x53535243 && read<std::uint16_t>(encoded, 6) == 64);
    assert(read<std::int32_t>(encoded, 24) == -1920 && read<std::uint64_t>(encoded, 56) == 1ull << 32);
    assert(read<std::uint32_t>(encoded, 16) == 127);
    auto stale = status;
    constrain_readiness(stale, true, status.generation + 1);
    assert(stale.generation == status.generation && stale.flags == 15);
    stale = status;
    constrain_readiness(stale, false, status.generation);
    assert(stale.flags == 15); // A reconnect cannot reuse a cached acknowledgement.
    stale = status;
    constrain_readiness(stale, true, status.generation);
    assert(stale.flags == status.flags);
    // A resize continuation must never publish an older visible frame after Off.
    crosshair::LatestFrame latest;
    Frame resizing{{1, 32, 32, 128, 960, 540, visible | center}, {}, 1};
    assert(latest.observe(resizing) && latest.matches(resizing));
    Frame off{{2, 0, 0, 0, 0, 0, 0}, {}, 1};
    assert(latest.observe(off) && !latest.matches(resizing) && latest.matches(off));
    Frame old_close{{1, 0, 0, 0, 0, 0, close}, {}, 1};
    assert(!latest.observe(old_close) && !latest.matches(old_close));
    Frame reconnect{{1, 32, 32, 128, 960, 540, visible}, {}, 3};
    assert(latest.observe(reconnect) && !latest.matches(off));
    assert(!latest.observe(resizing));
    // Small -> large without a recenter flag must request a larger window.
    crosshair::PhysicalBounds small{860, 490, 1060, 590};
    assert(crosshair::fits(resizing.header, small));
    auto large = reconnect.header; large.width = large.height = 800;
    assert(!(large.flags & center) && !crosshair::fits(large, small));
    auto size = crosshair::desired_size(large, small, 1);
    assert(size.width >= 800 && size.height >= 800);
    crosshair::PhysicalBounds grown{960 - std::int64_t(size.width / 2), 540 - std::int64_t(size.height / 2),
        960 + std::int64_t(size.width / 2), 540 + std::int64_t(size.height / 2)};
    assert(crosshair::fits(large, grown));
    assert(!crosshair::fits(large, small)); // A refused resize stays unpositioned.
    assert(large.center_x == 960 && large.center_y == 540);
    // Preparation acknowledges a fitted bitmap without ever enabling its image.
    crosshair::BitmapState bitmap;
    bitmap.position(preparing, true);
    assert(bitmap.ready && !bitmap.shown);
    assert(bitmap.flags(true, true, true) == prepared);
    assert(bitmap.flags(true, false, false) == prepared); // Shell visibility is independent.
    assert(bitmap.flags(false, true, true) == 0); // Superseded generations cannot acknowledge readiness.
    bitmap.clear(); // New work, Off, close and disconnect revoke both flags.
    assert(bitmap.flags(true, true, true) == 0);
    bitmap.position(preparing, false); // Refused resize cannot be ready.
    assert(!bitmap.ready && !bitmap.shown);
    auto activation = preparing; activation.flags = visible; ++activation.generation;
    bitmap.position(activation, true);
    assert(bitmap.shown && bitmap.flags(true, true, true) == (prepared | presented));
    assert(bitmap.flags(true, false, true) == prepared);
    assert(bitmap.flags(true, true, false) == prepared);
    bitmap.position(off.header, true);
    assert(!bitmap.ready && !bitmap.shown);
    bitmap.position(old_close.header, true);
    assert(!bitmap.ready && !bitmap.shown);
    crosshair::LatestFrame transition;
    Frame preparation{preparing, {}, 7}, activation_frame{activation, {}, 7};
    assert(transition.observe(preparation));
    assert(transition.observe(activation_frame) && !transition.matches(preparation));
    assert(!transition.observe(preparation)); // Late prepare completion cannot blank activation.
    bitmap.position(activation, true);
    assert(bitmap.flags(transition.matches(preparation), true, true) == 0);
    std::cout << "Widget protocol, hidden preparation, superseded presentation and resize tests passed\n";
}
