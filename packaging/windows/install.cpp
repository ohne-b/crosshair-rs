#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <sddl.h>
#include <bcrypt.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Management.Deployment.h>
#include <winrt/Windows.ApplicationModel.h>
#include <winrt/Windows.System.h>
#include <array>
#include <filesystem>
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

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    bool quiet = false;
    try {
        init_apartment(apartment_type::multi_threaded);
        int argc{};
        auto argv = CommandLineToArgvW(GetCommandLineW(), &argc);
        if (!argv) throw_last_error();
        std::vector<std::wstring> args(argv, argv + argc);
        LocalFree(argv);
        auto verify_only = args.size() == 2 && args[1] == L"--verify";
        quiet = verify_only || (args.size() > 1 && args[1].ends_with(L"-silent"));
        auto elevated = args.size() == 3 && (args[1] == L"--elevated" || args[1] == L"--elevated-silent");
        if (!(verify_only || (args.size() == 2 && (args[1] == L"--install" || args[1] == L"--install-silent")) || elevated))
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
        if (elevated && args[2] != user_sid())
            throw hresult_error(E_ACCESSDENIED, L"Approve installation using the same Windows account.");
        if (IsUserAnAdmin()) { deploy(directory); return 0; }
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
            message += to_message();
            message += code;
            MessageBoxW(nullptr, message.c_str(), L"crosshair-rs Setup", MB_OK | MB_ICONERROR);
        }
        return static_cast<int>(error);
    }
}
