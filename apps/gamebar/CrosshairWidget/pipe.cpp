#include "pipe.h"
#include <appmodel.h>
#include <algorithm>
#include <memory>

namespace crosshair {
namespace {
struct HandleCloser { void operator()(void* handle) const { if (handle && handle != INVALID_HANDLE_VALUE) CloseHandle(handle); } };
using Handle = std::unique_ptr<void, HandleCloser>;

std::vector<std::byte> user(HANDLE process) {
    HANDLE raw{};
    if (!OpenProcessToken(process, TOKEN_QUERY, &raw)) return {};
    Handle token{raw};
    DWORD length{};
    GetTokenInformation(token.get(), TokenUser, nullptr, 0, &length);
    if (!length || length > 65536) return {};
    std::vector<std::byte> bytes(length);
    if (!GetTokenInformation(token.get(), TokenUser, bytes.data(), length, &length)) return {};
    return bytes;
}
}

Pipe::Pipe(std::wstring package_name, std::wstring engine_path, std::uint32_t session,
           FrameHandler frame, ConnectionHandler connection)
    : package_name_(std::move(package_name)), engine_path_(std::move(engine_path)),
      pipe_name_(L"\\\\.\\pipe\\LOCAL\\crosshair-rs.widget.v1." + std::to_wstring(session)),
      session_(session), on_frame_(std::move(frame)), on_connection_(std::move(connection)) {
    stopped_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    status_changed_ = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!stopped_ || !status_changed_) {
        if (stopped_) CloseHandle(stopped_);
        if (status_changed_) CloseHandle(status_changed_);
        throw std::runtime_error("Cannot create widget connection events");
    }
}

Pipe::~Pipe() { stop(); CloseHandle(stopped_); CloseHandle(status_changed_); }
void Pipe::start() { if (!worker_.joinable()) { ResetEvent(stopped_); worker_ = std::thread([this] { run(); }); } }
void Pipe::stop() {
    SetEvent(stopped_);
    if (worker_.joinable()) worker_.join();
}
void Pipe::status(wire::Status value, std::uint64_t connection) {
    { std::lock_guard lock(status_mutex_); status_ = value; status_connection_ = connection; }
    SetEvent(status_changed_);
}

bool Pipe::authenticate(HANDLE pipe) const {
    ULONG pid{};
    DWORD session{};
    if (!GetNamedPipeServerProcessId(pipe, &pid) || !ProcessIdToSessionId(pid, &session) || session != session_) return false;
    Handle process{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)};
    if (!process) return false;
    UINT32 count{};
    if (GetPackageFullName(process.get(), &count, nullptr) != ERROR_INSUFFICIENT_BUFFER || count > 4096) return false;
    std::wstring package(count, L'\0');
    if (GetPackageFullName(process.get(), &count, package.data()) != ERROR_SUCCESS) return false;
    package.resize(count - 1);
    if (package != package_name_) return false;
    DWORD path_length = 32768;
    std::wstring path(path_length, L'\0');
    if (!QueryFullProcessImageNameW(process.get(), 0, path.data(), &path_length)) return false;
    path.resize(path_length);
    if (CompareStringOrdinal(path.c_str(), -1, engine_path_.c_str(), -1, TRUE) != CSTR_EQUAL) return false;
    auto server_user = user(process.get()), own_user = user(GetCurrentProcess());
    return !server_user.empty() && !own_user.empty() &&
        EqualSid(reinterpret_cast<TOKEN_USER*>(server_user.data())->User.Sid,
                 reinterpret_cast<TOKEN_USER*>(own_user.data())->User.Sid);
}

bool Pipe::transfer(HANDLE pipe, std::span<std::byte> bytes, bool writing, DWORD deadline) {
    Handle completed{CreateEventW(nullptr, TRUE, FALSE, nullptr)};
    if (!completed) return false;
    auto start = GetTickCount64();
    while (!bytes.empty()) {
        OVERLAPPED operation{}; operation.hEvent = completed.get();
        ResetEvent(completed.get());
        auto count = static_cast<DWORD>(bytes.size());
        BOOL immediate = writing ? WriteFile(pipe, bytes.data(), count, nullptr, &operation)
                                 : ReadFile(pipe, bytes.data(), count, nullptr, &operation);
        if (!immediate && GetLastError() != ERROR_IO_PENDING) return false;
        HANDLE events[]{stopped_, completed.get(), status_changed_};
        for (;;) {
            auto elapsed = GetTickCount64() - start;
            DWORD remaining = deadline == INFINITE ? INFINITE : elapsed >= deadline ? 0 : deadline - static_cast<DWORD>(elapsed);
            DWORD wait = WaitForMultipleObjects(writing ? 2 : 3, events, FALSE, remaining);
            if (wait == WAIT_OBJECT_0 + 1) break;
            if (!writing && wait == WAIT_OBJECT_0 + 2 && write_status(pipe)) continue;
            CancelIoEx(pipe, &operation);
            DWORD ignored{}; GetOverlappedResult(pipe, &operation, &ignored, TRUE);
            return false;
        }
        DWORD transferred{};
        if (!GetOverlappedResult(pipe, &operation, &transferred, FALSE) || !transferred) return false;
        bytes = bytes.subspan(transferred);
        // An idle header can wait forever; a partial frame cannot hold the connection forever.
        if (!writing && deadline == INFINITE && !bytes.empty()) { deadline = 5000; start = GetTickCount64(); }
    }
    return true;
}

bool Pipe::write_status(HANDLE pipe) {
    wire::Status value;
    std::uint64_t connection{};
    { std::lock_guard lock(status_mutex_); value = status_; connection = status_connection_; }
    value.flags = connected() ? value.flags | 1u : value.flags & ~1u;
    // The UI may still be awaiting resize or may have queued an older report.
    // Never send readiness for that superseded generation or a new connection.
    wire::constrain_readiness(value, have_generation_ && connection == epoch(), received_generation_);
    auto bytes = wire::encode(value);
    return transfer(pipe, bytes, true, 2000);
}

void Pipe::run() {
    DWORD backoff = 250;
    while (WaitForSingleObject(stopped_, 0) != WAIT_OBJECT_0) {
        Handle pipe{CreateFileW(pipe_name_.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
            OPEN_EXISTING, FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, nullptr)};
        if (pipe.get() != INVALID_HANDLE_VALUE && pipe && authenticate(pipe.get())) {
            have_generation_ = false;
            connected_ = true; ++epoch_; on_connection_(); backoff = 250;
            bool healthy = write_status(pipe.get());
            while (healthy && WaitForSingleObject(stopped_, 0) != WAIT_OBJECT_0) {
                try {
                    std::array<std::byte, wire::header_size> bytes{};
                    if (!transfer(pipe.get(), bytes, false, INFINITE)) break;
                    wire::Frame frame{wire::decode(bytes), {}, epoch()};
                    frame.pixels.resize(frame.header.byte_count());
                    if (!frame.pixels.empty() && !transfer(pipe.get(), frame.pixels, false, 5000)) break;
                    if (have_generation_ && frame.header.generation <= received_generation_) continue;
                    received_generation_ = frame.header.generation; have_generation_ = true;
                    on_frame_(std::move(frame));
                } catch (...) { healthy = false; }
            }
            have_generation_ = false;
            connected_ = false; ++epoch_; on_connection_();
        }
        if (WaitForSingleObject(stopped_, backoff) == WAIT_OBJECT_0) break;
        backoff = std::min<DWORD>(backoff * 2, 4000);
    }
}
}
