#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <stdexcept>
#include <type_traits>
#include <vector>

namespace crosshair::wire {
// Byte-mode pipe, little-endian fields, no C++ object layout on the wire.
// Engine: CRSF, u16 version/size, u64 generation, u32 width/height/stride,
// i32 physical center x/y, u32 flags, then stride*height premultiplied BGRA.
// Widget: CRSS, u16 version/size, followed by the fields encoded in Status.
// A hidden/close frame has zero dimensions and no body. Generations increase
// within each authenticated connection; neither message carries trusted identity.
// prepare retains visible's full-payload requirement but must never show pixels.
inline constexpr std::uint16_t version = 1;
inline constexpr std::size_t header_size = 40;
inline constexpr std::size_t status_size = 64;
inline constexpr std::uint32_t maximum_dimension = 1024;
inline constexpr std::uint32_t visible = 1;
inline constexpr std::uint32_t center = 2;
inline constexpr std::uint32_t close = 4;
inline constexpr std::uint32_t prepare = 8;
inline constexpr std::uint32_t prepared = 32;
inline constexpr std::uint32_t presented = 64;

template<class T> T read(std::span<std::byte const> bytes, std::size_t offset) {
    using U = std::make_unsigned_t<T>;
    if (offset + sizeof(T) > bytes.size()) throw std::runtime_error("Truncated widget message");
    U value{};
    for (std::size_t i = 0; i < sizeof(T); ++i)
        value |= static_cast<U>(std::to_integer<unsigned char>(bytes[offset + i])) << (i * 8);
    return static_cast<T>(value);
}

template<class T> void write(std::span<std::byte> bytes, std::size_t offset, T value) {
    using U = std::make_unsigned_t<T>;
    for (std::size_t i = 0; i < sizeof(T); ++i)
        bytes[offset + i] = static_cast<std::byte>((static_cast<U>(value) >> (i * 8)) & 255);
}

struct Header {
    std::uint64_t generation{};
    std::uint32_t width{}, height{}, stride{};
    std::int32_t center_x{}, center_y{};
    std::uint32_t flags{};

    std::size_t byte_count() const { return static_cast<std::size_t>(stride) * height; }
};

inline Header decode(std::span<std::byte const> bytes) {
    if (bytes.size() != header_size || read<std::uint32_t>(bytes, 0) != 0x46535243 ||
        read<std::uint16_t>(bytes, 4) != version || read<std::uint16_t>(bytes, 6) != header_size)
        throw std::runtime_error("Incompatible widget protocol");
    Header h{read<std::uint64_t>(bytes, 8), read<std::uint32_t>(bytes, 16),
        read<std::uint32_t>(bytes, 20), read<std::uint32_t>(bytes, 24),
        read<std::int32_t>(bytes, 28), read<std::int32_t>(bytes, 32), read<std::uint32_t>(bytes, 36)};
    if (h.flags & ~(visible | center | close | prepare)) throw std::runtime_error("Unknown widget flags");
    if ((h.flags & close) && h.flags != close) throw std::runtime_error("Invalid close frame flags");
    if ((h.flags & (prepare | center)) && !(h.flags & visible)) throw std::runtime_error("Geometry flags require a raster payload");
    if (!(h.flags & visible)) {
        if (h.width || h.height || h.stride) throw std::runtime_error("Hidden frame has pixels");
    } else if (!h.width || !h.height || h.width > maximum_dimension ||
               h.height > maximum_dimension || h.stride != h.width * 4) {
        throw std::runtime_error("Invalid widget dimensions");
    }
    return h;
}

struct Frame {
    Header header;
    std::vector<std::byte> pixels;
    std::uint64_t connection{};
};

struct Status {
    std::uint64_t generation{};
    std::uint32_t flags{}, dpi{96};
    std::int32_t left{}, top{};
    std::uint32_t width{}, height{};
    std::int32_t center_x{}, center_y{};
    std::uint32_t process_id{}, session_id{};
    std::uint64_t package_version{};
};
// Status bits: connected=1, pinned=2, shell-visible=4, click-through=8,
// positioned=16, exact-generation bitmap prepared=32, bitmap presented=64.
// Prepared is independent of shell visibility; a prepare frame never presents.
inline void constrain_readiness(Status& status, bool have_generation, std::uint64_t latest_generation) {
    if (!have_generation || status.generation != latest_generation)
        status.flags &= ~(16u | prepared | presented);
}

inline std::array<std::byte, status_size> encode(Status const& status) {
    std::array<std::byte, status_size> bytes{};
    write<std::uint32_t>(bytes, 0, 0x53535243);
    write<std::uint16_t>(bytes, 4, version);
    write<std::uint16_t>(bytes, 6, status_size);
    write(bytes, 8, status.generation); write(bytes, 16, status.flags); write(bytes, 20, status.dpi);
    write(bytes, 24, status.left); write(bytes, 28, status.top);
    write(bytes, 32, status.width); write(bytes, 36, status.height);
    write(bytes, 40, status.center_x); write(bytes, 44, status.center_y);
    write(bytes, 48, status.process_id); write(bytes, 52, status.session_id);
    write(bytes, 56, status.package_version);
    return bytes;
}
}
