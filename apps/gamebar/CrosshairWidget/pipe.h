#pragma once

#include "wire.h"
#include <windows.h>
#include <atomic>
#include <functional>
#include <mutex>
#include <string>
#include <thread>

namespace crosshair {
class Pipe {
public:
    using FrameHandler = std::function<void(wire::Frame)>;
    using ConnectionHandler = std::function<void()>;
    Pipe(std::wstring package_name, std::wstring engine_path, std::uint32_t session,
         FrameHandler frame, ConnectionHandler connection);
    ~Pipe();
    Pipe(Pipe const&) = delete;
    Pipe& operator=(Pipe const&) = delete;
    void start();
    void stop();
    void status(wire::Status value, std::uint64_t connection);
    bool connected() const { return connected_.load(); }
    std::uint64_t epoch() const { return epoch_.load(); }
private:
    void run();
    bool authenticate(HANDLE pipe) const;
    bool transfer(HANDLE pipe, std::span<std::byte> bytes, bool write, DWORD deadline);
    bool write_status(HANDLE pipe);
    std::wstring package_name_, engine_path_, pipe_name_;
    std::uint32_t session_;
    FrameHandler on_frame_;
    ConnectionHandler on_connection_;
    HANDLE stopped_{}, status_changed_{};
    std::thread worker_;
    std::atomic<bool> connected_{};
    std::atomic<std::uint64_t> epoch_{};
    std::mutex status_mutex_;
    wire::Status status_;
    std::uint64_t status_connection_{};
    // Accessed only by the pipe worker, including nested status writes.
    bool have_generation_{};
    std::uint64_t received_generation_{};
};
}
