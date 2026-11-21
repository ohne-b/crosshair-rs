#include <windows.h>
#include <string_view>
#include <shellapi.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <shobjidl.h>
#include <propkey.h>
#include <propvarutil.h>
#include <sddl.h>
#include <bcrypt.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Management.Deployment.h>
#include <winrt/Windows.ApplicationModel.h>
#include <winrt/Windows.System.h>
#include <array>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>
#include "payload.h"

namespace fs = std::filesystem;
using namespace winrt;
using namespace Windows::Management::Deployment;
constexpr wchar_t framework_hash[] = L"9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283";

winrt::handle lock_file(fs::path const& path, bool directory = false) {
    auto h = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
        OPEN_EXISTING, directory ? FILE_FLAG_BACKUP_SEMANTICS : FILE_ATTRIBUTE_NORMAL, nullptr);
    check_bool(h != INVALID_HANDLE_VALUE);
    return winrt::handle(h);
}

void verify(HANDLE file, std::wstring const& expected) {
    LARGE_INTEGER zero{};
    check_bool(SetFilePointerEx(file, zero, nullptr, FILE_BEGIN));
    struct Hash {
        BCRYPT_HASH_HANDLE value{};
        ~Hash() { if (value) BCryptDestroyHash(value); }
    } hash;
    check_nt(BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &hash.value, nullptr, 0, nullptr, 0, 0));
    std::array<UCHAR, 65536> buffer{};
    DWORD count{};
    unsigned long long total{};
    for (;;) {
        check_bool(ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &count, nullptr));
        if (!count) break;
        total += count;
        if (total > 512ull * 1024 * 1024) throw hresult_error(E_FAIL, L"Installer payload exceeds its size limit.");
        check_nt(BCryptHashData(hash.value, buffer.data(), count, 0));
    }
    std::array<UCHAR, 32> digest{};
    check_nt(BCryptFinishHash(hash.value, digest.data(), static_cast<ULONG>(digest.size()), 0));
    std::wstring actual;
    for (auto byte : digest) {
        actual += L"0123456789abcdef"[byte >> 4];
        actual += L"0123456789abcdef"[byte & 15];
    }
    if (actual != expected) throw hresult_error(E_FAIL, L"The installer payload is damaged. Download the setup again.");
}

std::wstring user_sid() {
    HANDLE raw{};
    check_bool(OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw));
    winrt::handle token(raw);
    DWORD size{};
    GetTokenInformation(token.get(), TokenUser, nullptr, 0, &size);
    std::vector<BYTE> bytes(size);
    check_bool(GetTokenInformation(token.get(), TokenUser, bytes.data(), size, &size));
    LPWSTR text{};
    check_bool(ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(bytes.data())->User.Sid, &text));
    std::wstring result(text);
    LocalFree(text);
    return result;
}

Windows::Foundation::Uri file_uri(fs::path const& path) {
    std::array<wchar_t, 32768> url{};
    DWORD length = static_cast<DWORD>(url.size());
    check_hresult(UrlCreateFromPathW(path.c_str(), url.data(), &length, 0));
    return Windows::Foundation::Uri(url.data());
}

bool framework_installed(PackageManager const& manager) {
    for (auto const& package : manager.FindPackagesForUser(L"", L"Microsoft.VCLibs.140.00",
        L"CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US")) {
        auto id = package.Id();
        auto v = id.Version();
        auto version = (uint64_t(v.Major) << 48) | (uint64_t(v.Minor) << 32) | (uint64_t(v.Build) << 16) | v.Revision;
        if (id.Architecture() == Windows::System::ProcessorArchitecture::X64 &&
            version >= ((14ull << 48) | (33519ull << 16)) && package.Status().VerifyIsOK()) return true;
    }
    return false;
}

struct Staging {
    fs::path path;
    Staging() {
        PWSTR common{};
        check_hresult(SHGetKnownFolderPath(FOLDERID_ProgramData, 0, nullptr, &common));
        fs::path root(common);
        CoTaskMemFree(common);
        GUID id{};
        check_hresult(CoCreateGuid(&id));
        wchar_t name[40]{};
        StringFromGUID2(id, name, 40);
        path = root / (std::wstring(L"crosshair-rs-setup-") + name);
        PSECURITY_DESCRIPTOR descriptor{};
        check_bool(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            L"O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)", SDDL_REVISION_1, &descriptor, nullptr));
        SECURITY_ATTRIBUTES attributes{sizeof(attributes), descriptor, FALSE};
        auto created = CreateDirectoryW(path.c_str(), &attributes);
        auto error = GetLastError();
        LocalFree(descriptor);
        if (!created) throw hresult_error(HRESULT_FROM_WIN32(error));
    }
    ~Staging() {
        DeleteFileW((path / L"package.msix").c_str());
        DeleteFileW((path / L"framework.appx").c_str());
        RemoveDirectoryW(path.c_str());
    }
};

void deploy(fs::path const& source) {
    Staging staging;
    auto package = staging.path / L"package.msix";
    check_bool(CopyFileW((source / L"package.msix").c_str(), package.c_str(), TRUE));
    auto locked = lock_file(package);
    verify(locked.get(), payload_hash);
    PackageManager manager;
    AddPackageOptions options;
    options.AllowUnsigned(true);
    winrt::handle dependency;
    if (!framework_installed(manager)) {
        auto framework = staging.path / L"framework.appx";
        check_bool(CopyFileW((source / L"Microsoft.VCLibs.x64.14.00.appx").c_str(), framework.c_str(), TRUE));
        dependency = lock_file(framework);
        verify(dependency.get(), framework_hash);
        options.DependencyPackageUris().Append(file_uri(framework));
    }
    auto operation = manager.AddPackageByUriAsync(file_uri(package), options);
    auto result = operation.get();
    if (FAILED(result.ExtendedErrorCode())) throw hresult_error(result.ExtendedErrorCode(), result.ErrorText());
}

Windows::ApplicationModel::Package installed() {
    PackageManager manager;
    for (auto const& package : manager.FindPackagesForUser(L"", payload_name, payload_publisher)) {
        auto id = package.Id();
        auto v = id.Version();
        auto version = std::to_wstring(v.Major) + L"." + std::to_wstring(v.Minor) + L"." +
            std::to_wstring(v.Build) + L"." + std::to_wstring(v.Revision);
        if (id.Architecture() == Windows::System::ProcessorArchitecture::X64 &&
            version == payload_version && package.Status().VerifyIsOK()) return package;
    }
    throw hresult_error(E_FAIL, L"Windows has not registered this version of crosshair-rs.");
}

DWORD activate(std::wstring const& app, std::wstring const& arguments) {
    auto manager = create_instance<IApplicationActivationManager>(CLSID_ApplicationActivationManager, CLSCTX_LOCAL_SERVER);
    DWORD process{};
    check_hresult(manager->ActivateApplication(app.c_str(), arguments.c_str(), AO_NONE, &process));
    return process;
}

void enable_startup(Windows::ApplicationModel::Package const& package) {
    PWSTR local{};
    check_hresult(SHGetKnownFolderPath(FOLDERID_LocalAppData, 0, nullptr, &local));
    auto directory = fs::path(local) / L"Packages" / std::wstring(package.Id().FamilyName()) / L"LocalState";
    CoTaskMemFree(local);
    // A new reply in setup's AppData temp folder can be redirected away from the unpackaged reader.
    fs::create_directories(directory);
    GUID id{};
    check_hresult(CoCreateGuid(&id));
    wchar_t name[40]{};
    StringFromGUID2(id, name, 40);
    auto reply = directory / (std::wstring(L"startup-") + name + L".txt");
    auto arguments = std::wstring(L"--configure-startup true --reply-file \"") + reply.wstring() + L"\"";
    activate(std::wstring(package.Id().FamilyName()) + L"!Engine", arguments);
    // Activation may return a resident engine; completion is the atomic reply, not process exit.
    auto deadline = GetTickCount64() + 30000;
    while (!fs::exists(reply)) {
        if (GetTickCount64() >= deadline)
            throw hresult_error(HRESULT_FROM_WIN32(ERROR_TIMEOUT), L"Windows did not finish changing launch at sign-in.");
        Sleep(50);
    }
    if (fs::file_size(reply) > 16384)
        throw hresult_error(E_FAIL, L"Windows did not return a valid startup configuration result.");
    std::ifstream stream(reply, std::ios::binary);
    std::string result((std::istreambuf_iterator<char>(stream)), std::istreambuf_iterator<char>());
    stream.close();
    check_bool(DeleteFileW(reply.c_str()));
    if (result != "ok") {
        if (result.starts_with("error\n")) throw hresult_error(E_FAIL, to_hstring(result.substr(6)));
        throw hresult_error(E_FAIL, L"Launch at sign-in could not be configured. Open crosshair-rs Settings and try again.");
    }
}

fs::path shortcut_path(REFKNOWNFOLDERID folder, std::wstring const& name) {
    PWSTR raw{};
    check_hresult(SHGetKnownFolderPath(folder, 0, nullptr, &raw));
    fs::path path = fs::path(raw) / (name + L".lnk");
    CoTaskMemFree(raw);
    return path;
}

bool owns_shortcut(fs::path const& path, std::wstring const& app) {
    com_ptr<IShellItem2> item;
    if (FAILED(SHCreateItemFromParsingName(path.c_str(), nullptr, IID_PPV_ARGS(item.put())))) return false;
    for (auto const& key : {PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath}) {
        PWSTR value{};
        auto status = item->GetString(key, &value);
        auto matches = SUCCEEDED(status) && value && app == value;
        CoTaskMemFree(value);
        if (!matches) return false;
    }
    return true;
}

std::wstring shortcut_name() {
    return std::wstring_view(payload_name).ends_with(L".Preview") ? L"crosshair-rs Preview" : L"crosshair-rs";
}

int shortcut_defaults() {
    PackageManager manager;
    auto packages = manager.FindPackagesForUser(L"", payload_name, payload_publisher);
    auto current = packages.First();
    auto name = shortcut_name();
    auto path = shortcut_path(FOLDERID_Desktop, name);
    if (!fs::exists(path)) return !current.HasCurrent();
    if (!current.HasCurrent()) return 2; // Leave an existing link alone until Windows can resolve its target.
    return owns_shortcut(path, std::wstring(current.Current().Id().FamilyName()) + L"!Editor");
}

fs::path shortcut_icon(Windows::ApplicationModel::Package const& package) {
    PWSTR local{};
    check_hresult(SHGetKnownFolderPath(FOLDERID_LocalAppData, 0, nullptr, &local));
    auto shell = fs::path(local) / L"Packages" / std::wstring(package.Id().FamilyName()) / L"LocalState" / L"Shell";
    CoTaskMemFree(local);
    fs::create_directories(shell);
    auto icon = shell / L"crosshair.ico";
    fs::copy_file(fs::path(package.InstalledPath().c_str()) / L"Assets" / L"app.ico", icon, fs::copy_options::skip_existing);
    return icon;
}

void shortcut(REFKNOWNFOLDERID folder, std::wstring const& name, std::wstring const& app,
              bool enabled, Windows::ApplicationModel::Package const& package) {
    auto path = shortcut_path(folder, name);
    if (fs::exists(path)) {
        if (!owns_shortcut(path, app)) {
            if (enabled) throw hresult_error(E_FAIL, L"A different shortcut already uses the crosshair-rs name.");
            return;
        }
        if (!enabled) {
            check_bool(DeleteFileW(path.c_str()));
            SHChangeNotify(SHCNE_DELETE, SHCNF_PATHW, path.c_str(), nullptr);
        }
        return;
    }
    if (!enabled) {
        return;
    }
    PIDLIST_ABSOLUTE target{};
    auto parsing_name = std::wstring(L"shell:AppsFolder\\") + app;
    check_hresult(SHParseDisplayName(parsing_name.c_str(), nullptr, &target, 0, nullptr));
    struct Pidl { PIDLIST_ABSOLUTE value; ~Pidl() { CoTaskMemFree(value); } } expected{target};
    auto link = create_instance<IShellLinkW>(CLSID_ShellLink);
    check_hresult(link->SetIDList(target));
    check_hresult(link->SetDescription(name.c_str()));
    check_hresult(link->SetIconLocation(shortcut_icon(package).c_str(), 0));
    auto properties = link.as<IPropertyStore>();
    PROPVARIANT identity{};
    check_hresult(InitPropVariantFromString(app.c_str(), &identity));
    auto status = properties->SetValue(PKEY_AppUserModel_ID, identity);
    PropVariantClear(&identity);
    check_hresult(status);
    check_hresult(properties->Commit());
    check_hresult(link.as<IPersistFile>()->Save(path.c_str(), TRUE));
    SHChangeNotify(SHCNE_CREATE, SHCNF_PATHW, path.c_str(), nullptr);
}

bool option(std::wstring const& value) {
    if (value == L"0") return false;
    if (value == L"1") return true;
    throw hresult_invalid_argument();
}

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    bool quiet = false;
    try {
        init_apartment(apartment_type::multi_threaded);
        int argc{};
        auto argv = CommandLineToArgvW(GetCommandLineW(), &argc);
        if (!argv) throw_last_error();
        std::vector<std::wstring> args(argv, argv + argc);
        LocalFree(argv);
        if (args.size() == 2 && args[1] == L"--shortcut-defaults") {
            quiet = true;
            return shortcut_defaults();
        }
        auto verify_only = args.size() == 2 && args[1] == L"--verify";
        quiet = verify_only || (args.size() > 1 && args[1].ends_with(L"-silent"));
        auto elevated = args.size() == 3 && (args[1] == L"--elevated" || args[1] == L"--elevated-silent");
        auto apply_options = args.size() == 5 && (args[1] == L"--options" || args[1] == L"--options-silent");
        auto launch = args.size() == 2 && args[1] == L"--launch";
        if (!(verify_only || apply_options || launch || (args.size() == 2 && (args[1] == L"--install" || args[1] == L"--install-silent")) || elevated))
            throw hresult_invalid_argument();
        std::array<wchar_t, 32768> image{};
        auto length = GetModuleFileNameW(nullptr, image.data(), static_cast<DWORD>(image.size()));
        check_bool(length && length < image.size());
        fs::path executable(image.data());
        auto directory = executable.parent_path();
        auto self = lock_file(executable);
        auto folder = lock_file(directory, true);
        auto package = lock_file(directory / L"package.msix");
        auto framework = lock_file(directory / L"Microsoft.VCLibs.x64.14.00.appx");
        verify(package.get(), payload_hash);
        verify(framework.get(), framework_hash);
        if (verify_only) return 0;
        if (apply_options || launch) {
            auto package_info = installed();
            auto app = std::wstring(package_info.Id().FamilyName()) + L"!Editor";
            if (launch) { activate(app, L""); return 0; }
            if (args[2] != L"keep") option(args[2]);
            if (args[3] != L"keep") option(args[3]);
            auto startup = option(args[4]);
            // An unchecked option must not disable an existing sign-in preference on upgrade.
            if (startup) enable_startup(package_info);
            auto name = shortcut_name();
            if (args[2] != L"keep") shortcut(FOLDERID_Desktop, name, app, option(args[2]), package_info);
            if (args[3] != L"keep") shortcut(FOLDERID_Programs, name, app, option(args[3]), package_info);
            return 0;
        }
        if (elevated && args[2] != user_sid())
            throw hresult_error(E_ACCESSDENIED, L"Approve installation using the same Windows account.");
        if (IsUserAnAdmin()) {
            for (;;) {
                try { deploy(directory); return 0; }
                catch (hresult_error const& error) {
                    if (quiet || error.code() != HRESULT_FROM_WIN32(ERROR_PACKAGES_IN_USE)) throw;
                    std::wstring message = L"Save your edits, choose Quit from the crosshair-rs tray icon, then select Retry.\n\n";
                    message += error.message();
                    if (MessageBoxW(nullptr, message.c_str(), L"Close crosshair-rs to update",
                        MB_RETRYCANCEL | MB_ICONINFORMATION) != IDRETRY)
                        return static_cast<int>(HRESULT_FROM_WIN32(ERROR_CANCELLED));
                }
            }
        }
        if (elevated) throw hresult_error(E_ACCESSDENIED);
        auto parameters = std::wstring(quiet ? L"--elevated-silent " : L"--elevated ") + user_sid();
        SHELLEXECUTEINFOW execute{sizeof(execute)};
        execute.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        execute.lpVerb = L"runas";
        execute.lpFile = executable.c_str();
        execute.lpParameters = parameters.c_str();
        execute.nShow = SW_HIDE;
        check_bool(ShellExecuteExW(&execute));
        winrt::handle child(execute.hProcess);
        check_bool(WaitForSingleObject(child.get(), INFINITE) == WAIT_OBJECT_0);
        DWORD code{};
        check_bool(GetExitCodeProcess(child.get(), &code));
        return static_cast<int>(code);
    } catch (...) {
        auto error = to_hresult();
        if (!quiet) {
            wchar_t code[32]{};
            swprintf_s(code, L"\n\nWindows error: 0x%08X", static_cast<unsigned>(error));
            std::wstring message = L"Installation did not finish.\n\n";
            if (error == HRESULT_FROM_WIN32(ERROR_INSTALL_PACKAGE_DOWNGRADE))
                message += L"A newer version is already installed. Open it from Start or use a newer setup file.\n\n";
            else if (error == HRESULT_FROM_WIN32(ERROR_PACKAGE_ALREADY_EXISTS))
                message += L"Windows already has a different build with this version number. Use a newer setup to keep your settings.\n\n";
            message += to_message();
            message += code;
            MessageBoxW(nullptr, message.c_str(), L"crosshair-rs Setup", MB_OK | MB_ICONERROR);
        }
        return static_cast<int>(error);
    }
}
