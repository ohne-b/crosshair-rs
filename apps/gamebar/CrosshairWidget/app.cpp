#include <windows.h>
#undef GetCurrentTime
#include <CoreWindow.h>
#include <robuffer.h>
#include <winrt/Windows.ApplicationModel.h>
#include <winrt/Windows.ApplicationModel.Activation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Graphics.Display.h>
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.Storage.Streams.h>
#include <winrt/Windows.System.h>
#include <winrt/Windows.UI.h>
#include <winrt/Windows.UI.Core.h>
#include <winrt/Windows.UI.Xaml.h>
#include <winrt/Windows.UI.Xaml.Controls.h>
#include <winrt/Windows.UI.Xaml.Media.h>
#include <winrt/Windows.UI.Xaml.Media.Imaging.h>
#include <winrt/Microsoft.Gaming.XboxGameBar.h>
#include "pipe.h"
#include "presentation.h"
#include <algorithm>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <optional>

using namespace winrt;
using namespace winrt::Windows::ApplicationModel;
using namespace winrt::Windows::ApplicationModel::Activation;
using namespace winrt::Windows::Foundation;
using namespace winrt::Windows::Graphics::Display;
using namespace winrt::Windows::UI::Core;
using namespace winrt::Windows::UI::Xaml;
using namespace winrt::Windows::UI::Xaml::Controls;
using namespace winrt::Windows::UI::Xaml::Media;
using namespace winrt::Windows::UI::Xaml::Media::Imaging;
using namespace Microsoft::Gaming::XboxGameBar;

namespace crosshair {
struct App : ApplicationT<App> {
    App() {
        Suspending([this](auto const&, auto const&) { if (pipe_) pipe_->stop(); blank(); });
        Resuming([this](auto const&, auto const&) { if (widget_) update_state(); });
        UnhandledException([this](auto const&, UnhandledExceptionEventArgs const& event) {
            shutdown();
            OutputDebugStringW(event.Message().c_str());
        });
    }

    void OnLaunched(LaunchActivatedEventArgs const&) { open_editor(); }

    void OnActivated(IActivatedEventArgs const& args) {
        if (args.Kind() != ActivationKind::Protocol) return;
        auto protocol = args.try_as<IProtocolActivatedEventArgs>();
        if (!protocol || protocol.Uri().SchemeName() != L"ms-gamebarwidget") return;
        auto activation = args.try_as<XboxGameBarWidgetActivatedEventArgs>();
        if (!activation || !activation.IsLaunchActivation()) return;
        if (widget_) return;

        window_ = Window::Current();
        dispatcher_ = window_.Dispatcher();
        frame_ = Frame(); canvas_ = Canvas(); image_ = Image();
        canvas_.Background(SolidColorBrush(winrt::Windows::UI::Colors::Transparent()));
        canvas_.IsHitTestVisible(false);
        canvas_.UseLayoutRounding(true);
        image_.Stretch(Stretch::Fill);
        image_.IsHitTestVisible(false);
        canvas_.Children().Append(image_);
        frame_.Content(canvas_); window_.Content(frame_);
        widget_ = XboxGameBarWidget(activation, window_.CoreWindow(), frame_);
        widget_.PinningSupported(true);
        widget_.SettingsSupported(false);
        widget_.HorizontalResizeSupported(false);
        widget_.VerticalResizeSupported(false);
        widget_.MinWindowSize({32, 32});
        widget_.MaxWindowSize({4096, 4096});

        auto weak = get_weak();
        widget_.PinnedChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->update_state(); });
        widget_.VisibleChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->update_state(); });
        widget_.ClickThroughEnabledChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->report(); });
        widget_.RequestedOpacityChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) { self->image_.Opacity(self->widget_.RequestedOpacity() / 100.0); self->report(); } });
        widget_.WindowBoundsChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->reposition(); });
        window_.SizeChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->reposition(); });
        window_.Closed([weak](auto const&, auto const&) { if (auto self = weak.get()) { self->shutdown(); self->Exit(); } });
        display_ = DisplayInformation::GetForCurrentView();
        display_.DpiChanged([weak](auto const&, auto const&) { if (auto self = weak.get()) self->reposition(); });

        auto package = Package::Current();
        auto package_version = package.Id().Version();
        status_.package_version = (std::uint64_t(package_version.Major) << 48) |
            (std::uint64_t(package_version.Minor) << 32) | (std::uint64_t(package_version.Build) << 16) | package_version.Revision;
        status_.process_id = GetCurrentProcessId();
        DWORD session{};
        check_bool(ProcessIdToSessionId(status_.process_id, &session));
        status_.session_id = session;
        std::wstring engine = package.InstalledLocation().Path().c_str(); engine += L"\\crosshair-engine.exe";
        pipe_ = std::make_unique<Pipe>(package.Id().FullName().c_str(), std::move(engine), status_.session_id,
            [weak](wire::Frame value) noexcept {
                try { if (auto self = weak.get()) self->queue(std::move(value)); } catch (...) {}
            },
            [weak]() noexcept {
                try {
                    if (auto self = weak.get()) {
                        auto epoch = self->pipe_->epoch();
                        self->dispatcher_.RunAsync(CoreDispatcherPriority::Normal, [weak, epoch] {
                            if (auto app = weak.get()) {
                                if (app->presentation_epoch_ < epoch) {
                                    app->blank(); app->status_.generation = 0;
                                    app->presentation_epoch_ = epoch;
                                }
                                app->report();
                            }
                        });
                    }
                } catch (...) {}
            });
        window_.Activate();
        report(); update_state(); start_engine();
    }

private:
    fire_and_forget start_engine() {
        auto lifetime = get_strong();
        try { co_await FullTrustProcessLauncher::LaunchFullTrustProcessForCurrentAppAsync(); }
        catch (hresult_error const& error) { OutputDebugStringW(error.message().c_str()); }
    }

    fire_and_forget open_editor() {
        auto lifetime = get_strong();
        try {
            winrt::Windows::System::LauncherOptions options;
            options.TargetApplicationPackageFamilyName(Package::Current().Id().FamilyName());
            co_await winrt::Windows::System::Launcher::LaunchUriAsync(Uri(L"crosshair-rs://editor"), options);
        } catch (hresult_error const& error) { OutputDebugStringW(error.message().c_str()); }
        Exit();
    }

    void update_state() {
        if (!widget_ || !pipe_) return;
        if (widget_.Visible() || widget_.Pinned()) pipe_->start();
        else { pipe_->stop(); blank(); }
        report();
    }

    void shutdown() {
        if (pipe_) pipe_->stop();
        blank();
        widget_ = nullptr;
        std::lock_guard lock(pending_mutex_); pending_.reset();
    }

    void blank() {
        if (image_) { image_.Source(nullptr); image_.Visibility(Visibility::Collapsed); }
        current_.reset(); positioned_ = false; bitmap_state_.clear();
    }

    void queue(wire::Frame value) {
        bool schedule{};
        {
            std::lock_guard lock(pending_mutex_);
            if (closing_ || !latest_.observe(value)) return;
            pending_ = std::move(value); schedule = !presenting_; presenting_ = true;
        }
        if (schedule) {
            auto weak = get_weak();
            dispatcher_.RunAsync(CoreDispatcherPriority::Normal, [weak] { if (auto self = weak.get()) self->present(); });
        }
    }

    fire_and_forget present() {
        auto lifetime = get_strong();
        for (;;) {
            std::optional<wire::Frame> next;
            { std::lock_guard lock(pending_mutex_); next = std::move(pending_); pending_.reset(); if (!next) { presenting_ = false; co_return; } }
            try {
                if (!valid(*next)) continue;
                auto h = next->header;
                if (!(h.flags & wire::visible)) {
                    {
                        std::lock_guard lock(pending_mutex_);
                        if (!valid_locked(*next)) continue;
                        blank(); status_.generation = h.generation; presentation_epoch_ = next->connection;
                        closing_ = (h.flags & wire::close) != 0;
                    }
                    report();
                    if (closing_) { pipe_->stop(); widget_.Close(); co_return; }
                    continue;
                }
                {
                    std::lock_guard lock(pending_mutex_);
                    if (!valid_locked(*next)) continue;
                    // Do not expose an old bitmap during asynchronous preparation.
                    // New readiness belongs only to the completed incoming frame.
                    blank(); status_.generation = h.generation; presentation_epoch_ = next->connection;
                }
                report();
                double scale = display_.RawPixelsPerViewPixel();
                if (scale < 0.5 || scale > 8) throw hresult_invalid_argument();
                RECT rect{};
                bool known_bounds = bounds(rect);
                PhysicalBounds physical{rect.left, rect.top, rect.right, rect.bottom};
                if ((h.flags & wire::center) || !known_bounds || !fits(h, physical)) {
                    auto size = (h.flags & wire::center) || !known_bounds
                        ? DesiredSize{std::max(200.0f, float(h.width / scale + 16)), std::max(100.0f, float(h.height / scale + 16))}
                        : desired_size(h, physical, scale);
                    co_await widget_.TryResizeWindowAsync({size.width, size.height});
                    if (!valid(*next)) continue;
                }
                if (h.flags & wire::center) {
                    co_await widget_.CenterWindowAsync();
                    if (!valid(*next)) continue;
                }
                WriteableBitmap bitmap(static_cast<int>(h.width), static_cast<int>(h.height));
                auto buffer = bitmap.PixelBuffer();
                byte* destination{};
                check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
                if (buffer.Capacity() < next->pixels.size()) throw hresult_invalid_argument();
                std::memcpy(destination, next->pixels.data(), next->pixels.size());
                bitmap.Invalidate();
                {
                    std::lock_guard lock(pending_mutex_);
                    if (!valid_locked(*next)) continue;
                    image_.Source(bitmap);
                    image_.Opacity(widget_.RequestedOpacity() / 100.0);
                    current_ = h; status_.generation = h.generation; presentation_epoch_ = next->connection;
                }
                reposition();
            } catch (hresult_error const& error) { blank(); report(); OutputDebugStringW(error.message().c_str()); }
        }
    }

    bool valid(wire::Frame const& value) {
        std::lock_guard lock(pending_mutex_);
        return valid_locked(value);
    }

    bool valid_locked(wire::Frame const& value) const {
        return !closing_ && latest_.matches(value) && widget_ && pipe_ && pipe_->connected() && value.connection == pipe_->epoch();
    }

    bool bounds(RECT& rect) const {
        if (!window_) return false;
        auto interop = window_.CoreWindow().as<ICoreWindowInterop>();
        HWND hwnd{};
        if (FAILED(interop->get_WindowHandle(&hwnd)) || !GetClientRect(hwnd, &rect)) return false;
        POINT origin{};
        if (!ClientToScreen(hwnd, &origin)) return false;
        rect.right += origin.x; rect.bottom += origin.y; rect.left = origin.x; rect.top = origin.y;
        return true;
    }

    void reposition() {
        {
            std::lock_guard lock(pending_mutex_);
            if (widget_ && current_) {
                if (!current_is_latest_locked()) {
                    blank();
                } else {
                    auto h = *current_;
                    RECT rect{};
                    double scale = display_.RawPixelsPerViewPixel();
                    std::int64_t left = std::int64_t(h.center_x) - h.width / 2;
                    std::int64_t top = std::int64_t(h.center_y) - h.height / 2;
                    positioned_ = bounds(rect) && scale >= 0.5 && scale <= 8 &&
                        fits(h, {rect.left, rect.top, rect.right, rect.bottom});
                    if (positioned_) {
                        image_.Width(h.width / scale); image_.Height(h.height / scale);
                        Canvas::SetLeft(image_, (left - rect.left) / scale);
                        Canvas::SetTop(image_, (top - rect.top) / scale);
                        status_.center_x = h.center_x; status_.center_y = h.center_y;
                    }
                    bitmap_state_.position(h, positioned_);
                    image_.Visibility(bitmap_state_.shown ? Visibility::Visible : Visibility::Collapsed);
                }
            }
        }
        report();
    }

    bool current_is_latest_locked() const {
        return current_ && !closing_ && pipe_ && pipe_->connected() &&
            presentation_epoch_ == pipe_->epoch() && latest_.matches(presentation_epoch_, current_->generation);
    }

    void report() {
        if (!widget_ || !pipe_) return;
        bool latest{};
        { std::lock_guard lock(pending_mutex_); latest = current_is_latest_locked(); }
        status_.flags = (pipe_->connected() ? 1u : 0u) | (widget_.Pinned() ? 2u : 0u) |
            (widget_.Visible() ? 4u : 0u) | (widget_.ClickThroughEnabled() ? 8u : 0u) | (positioned_ && latest ? 16u : 0u) |
            bitmap_state_.flags(latest, widget_.Visible(), widget_.RequestedOpacity() > 0);
        status_.dpi = static_cast<std::uint32_t>(std::lround(display_.RawPixelsPerViewPixel() * 96));
        RECT rect{};
        if (bounds(rect)) {
            status_.left = rect.left; status_.top = rect.top;
            status_.width = rect.right - rect.left; status_.height = rect.bottom - rect.top;
        }
        pipe_->status(status_, presentation_epoch_);
    }

    Window window_{nullptr}; Frame frame_{nullptr}; Canvas canvas_{nullptr}; Image image_{nullptr};
    DisplayInformation display_{nullptr}; XboxGameBarWidget widget_{nullptr}; CoreDispatcher dispatcher_{nullptr};
    std::unique_ptr<Pipe> pipe_;
    wire::Status status_{};
    std::optional<wire::Header> current_;
    std::mutex pending_mutex_;
    std::optional<wire::Frame> pending_;
    LatestFrame latest_;
    BitmapState bitmap_state_;
    std::uint64_t presentation_epoch_{};
    bool presenting_{}, positioned_{}, closing_{};
};
}

int __stdcall wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    try {
        winrt::init_apartment(winrt::apartment_type::single_threaded);
        Application::Start([](auto const&) { winrt::make<crosshair::App>(); });
        return 0;
    } catch (winrt::hresult_error const& error) {
        OutputDebugStringW(error.message().c_str());
        return static_cast<int>(error.code());
    }
}
