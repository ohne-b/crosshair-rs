[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory)
$ErrorActionPreference = 'Stop'
# This test changes package registration only on the disposable hosted Windows runner.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Deployment acceptance runs only on a disposable GitHub-hosted runner.' }
$administrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (!$administrator.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'The deployment runner must already be elevated.' }
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
$identity = Read-AppxIdentity (Join-Path $ArtifactDirectory $build.package)
if ($identity.Name -notin @('OhneB.crosshair-rs', 'OhneB.crosshair-rs.Preview')) { throw 'Unexpected test package identity.' }
if (Get-AppxPackage -Name $identity.Name) { throw 'A package already exists; refusing to alter an existing installation.' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PackageActivation {
    [ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IActivation {
        [PreserveSig] int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string app,
            [MarshalAs(UnmanagedType.LPWStr)] string arguments, uint options, out uint process);
    }
    public static uint Start(string app, string arguments) {
        object manager = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("45ba127d-10a8-46ea-8ab7-56ea9078943c")));
        try { uint process; Marshal.ThrowExceptionForHR(((IActivation)manager).ActivateApplication(app, arguments, 0, out process)); return process; }
        finally { Marshal.ReleaseComObject(manager); }
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out IntPtr result);
    public static bool HasLargeIcon(IntPtr window) {
        IntPtr icon;
        return window != IntPtr.Zero && SendMessageTimeout(window, 0x007F, new UIntPtr(1), IntPtr.Zero, 2, 500, out icon) != IntPtr.Zero && icon != IntPtr.Zero;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool WaitNamedPipe(string name, uint timeout);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern int GetPackageFamilyName(IntPtr process, ref uint length, System.Text.StringBuilder family);
    public static bool HasPackage(System.Diagnostics.Process process, string expected) {
        uint length = 128;
        var family = new System.Text.StringBuilder((int)length);
        try { return GetPackageFamilyName(process.Handle, ref length, family) == 0 && family.ToString() == expected; }
        catch (System.InvalidOperationException) { return false; }
        catch (System.ComponentModel.Win32Exception) { return false; }
    }
    [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
    private static extern int DeriveAppContainerSidFromAppContainerName(string family, out IntPtr sid);
    [DllImport("advapi32.dll")]
    private static extern IntPtr FreeSid(IntPtr sid);
    public static string WidgetSid(string family) {
        IntPtr sid;
        Marshal.ThrowExceptionForHR(DeriveAppContainerSidFromAppContainerName(family, out sid));
        try { return new System.Security.Principal.SecurityIdentifier(sid).Value; }
        finally { FreeSid(sid); }
    }
}
'@
$fixtureRoot = [IO.Path]::GetFullPath((Join-Path $ArtifactDirectory ('deployment-' + [Guid]::NewGuid().ToString('N'))))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$shortcutName = if ($identity.Name.EndsWith('.Preview')) { 'crosshair-rs Preview.lnk' } else { 'crosshair-rs.lnk' }
$shortcuts = @([Environment]::GetFolderPath('DesktopDirectory'), [Environment]::GetFolderPath('Programs')) | ForEach-Object { Join-Path $_ $shortcutName }
foreach ($shortcut in $shortcuts) { if (Test-Path -LiteralPath $shortcut) { throw 'A shortcut already exists; refusing to replace it.' } }
$setup = (Resolve-Path -LiteralPath (Join-Path $ArtifactDirectory $build.setup)).Path
$helpers = @(Get-ChildItem -LiteralPath $ArtifactDirectory -Directory -Filter 'setup-*' | ForEach-Object { Join-Path $_.FullName 'install.exe' })
if ($helpers.Count -ne 1) { throw 'Expected one native helper from the tested setup build.' }
function Assert-ShortcutDefaults([int]$Expected) {
    $process = Start-Process -FilePath $helpers[0] -ArgumentList '--shortcut-defaults' -WindowStyle Hidden -PassThru
    try {
        if (!$process.WaitForExit(30000) -or $process.ExitCode -ne $Expected) { throw "Unexpected shortcut defaults; expected $Expected." }
    } finally { $process.Dispose() }
}
function Run-Setup([string]$Arguments) {
    $process = Start-Process -FilePath $setup -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    try {
        if (!$process.WaitForExit(180000)) { $process.Kill(); $process.WaitForExit(); throw 'Setup installation timed out.' }
        if ($process.ExitCode) { throw ('Setup failed for {0} (Windows error 0x{1:X8}).' -f $Arguments, $process.ExitCode) }
    } finally { $process.Dispose() }
}
function Activate-Request([string]$Arguments) {
    $reply = Join-Path $fixtureRoot ([Guid]::NewGuid().ToString('N') + '.txt')
    [PackageActivation]::Start($engineApp, ($Arguments + ' --reply-file "' + $reply + '"')) | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ($true) {
        if (Test-Path -LiteralPath $reply) {
            try {
                $content = Get-Content -LiteralPath $reply -Raw
                if ($Arguments.Contains('--command ')) {
                    $null = $content | ConvertFrom-Json -ErrorAction Stop
                    if (![string]::IsNullOrWhiteSpace($content)) { return $content }
                } elseif ($content -ceq 'ok' -or $content.StartsWith("error`n")) { return $content }
            } catch { } # A normal command reply can still be in the middle of its write.
        }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Packaged engine activation did not return a reply.' }
        Start-Sleep -Milliseconds 50
    }
}
function Assert-Startup([string]$Arguments, [bool]$Enabled, [string]$StatePath) {
    $value = $Enabled.ToString().ToLowerInvariant()
    $reply = Activate-Request ($Arguments + " --configure-startup $value")
    if ($reply -cne 'ok') { throw "Startup configuration failed: $reply" }
    $state = Get-Content -LiteralPath $StatePath -Raw | ConvertFrom-Json
    if ($state.settings.launch_at_login -ne $Enabled) { throw 'Startup configuration was not durably saved.' }
}
function Assert-WidgetWindow([bool]$Open) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $response = Activate-Request '--ipc-scope deployment-qa --command "{\"type\":\"subscribe\"}"' | ConvertFrom-Json
        $widget = $response.result.Ok.status.widget
        if ($null -ne $widget.window_open -and $widget.window_open -eq $Open -and !$widget.connected) { return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "The installed engine did not report window_open=$Open with the widget pipe disconnected."
}
function Get-BrokerCount {
    $brokers = @(Get-Process -Name RuntimeBroker -ErrorAction SilentlyContinue | Where-Object {
        $_.SessionId -eq $session -and [PackageActivation]::HasPackage($_, $installed.PackageFamilyName)
    })
    $brokers.Count
}
function Wait-ControlPipe {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (![PackageActivation]::WaitNamedPipe($pipeName, 100)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The isolated engine did not open its IPC endpoint.' }
        Start-Sleep -Milliseconds 50
    }
}
function Stop-TestEngine([int]$EngineId) {
    $quit = Activate-Request '--ipc-scope deployment-qa --command "{\"type\":\"quit\",\"discard_unsaved\":false}"'
    $response = $quit | ConvertFrom-Json
    if ($null -eq $response.result.PSObject.Properties['Ok']) { throw 'The isolated engine could not shut down.' }
    $resident = Get-Process -Id $EngineId -ErrorAction SilentlyContinue
    if ($resident) {
        try { if (!$resident.WaitForExit(15000)) { throw 'The isolated engine did not exit after acknowledging Quit.' } }
        finally { $resident.Dispose() }
    }
}
$installed = $null
$engineStarted = $false
try {
    Assert-ShortcutDefaults 1
    Run-Setup '/S /DESKTOP=1 /STARTUP=1'
    Assert-ShortcutDefaults 1
    $installed = Get-AppxPackage -Name $identity.Name
    if (!$installed -or $installed.Version -ne $build.packageVersion -or $installed.Publisher -cne $identity.Publisher -or $installed.Status -ne 'Ok') { throw 'Windows did not register the expected package.' }
    $engineApp = $installed.PackageFamilyName + '!Engine'
    $statePath = Join-Path $env:LOCALAPPDATA ('Packages/' + $installed.PackageFamilyName + '/LocalState/state.json')
    if (!(Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json).settings.launch_at_login) { throw 'The installer startup option was not saved.' }
    if (Get-ChildItem -LiteralPath (Split-Path -Parent $statePath) -Filter 'startup-*.txt') { throw 'Setup left its startup reply behind.' }
    $shell = New-Object -ComObject Shell.Application
    $editorApp = $installed.PackageFamilyName + '!Editor'
    $item = $shell.NameSpace((Split-Path -Parent $shortcuts[0])).ParseName($shortcutName)
    if (!$item -or $item.ExtendedProperty('System.AppUserModel.ID') -cne $editorApp) { throw 'Desktop shortcut does not use the stable packaged app identity.' }
    if (Test-Path -LiteralPath $shortcuts[1]) { throw 'Setup created a second Start menu entry.' }
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $entry = $shell.NameSpace('shell:AppsFolder').ParseName($editorApp)
        if ($entry -and $entry.ExtendedProperty('System.Tile.SmallLogoPath') -ceq 'Assets\Square44x44Logo.png') { break }
        if ([DateTime]::UtcNow -ge $deadline) { throw 'The editor app ID did not resolve to its packaged tile logo.' }
        Start-Sleep -Milliseconds 100
    } while ($true)
    $icon = Join-Path (Split-Path -Parent $statePath) 'Shell/crosshair.ico'
    $packedIcon = Join-Path $installed.InstallLocation 'Assets/app.ico'
    if ((Get-FileHash -LiteralPath $icon).Hash -cne (Get-FileHash -LiteralPath $packedIcon).Hash) { throw 'Shortcut icon differs from the installed artwork.' }
    $links = New-Object -ComObject WScript.Shell
    if ($links.CreateShortcut($shortcuts[0]).IconLocation -ine ($icon + ',0')) { throw 'Desktop shortcut has no explicit crosshair icon.' }
    Run-Setup '/S /STARTMENU=1'
    if (!(Test-Path -LiteralPath $shortcuts[1])) { throw 'An explicit legacy Start shortcut request failed.' }
    # Earlier setups left the icon field empty in both links.
    foreach ($shortcut in $shortcuts) {
        $old = $links.CreateShortcut($shortcut)
        $old.IconLocation = ',0'
        $old.Save()
    }
    Run-Setup '/S'
    foreach ($shortcut in $shortcuts) {
        if ($links.CreateShortcut($shortcut).IconLocation -ine ($icon + ',0')) { throw 'A silent upgrade did not repair an existing shortcut icon.' }
    }
    Run-Setup '/S /STARTMENU=0'
    if (Test-Path -LiteralPath $shortcuts[1]) { throw 'Explicitly clearing the legacy Start link failed.' }
    $custom = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcuts[0])
    $custom.Description = 'Custom crosshair shortcut'
    $custom.Save()
    $customHash = (Get-FileHash -LiteralPath $shortcuts[0]).Hash
    $ownedDesktop = Join-Path $fixtureRoot 'desktop.lnk'
    Copy-Item -LiteralPath $shortcuts[0] -Destination $ownedDesktop
    # Package uninstall can remove the cached artwork while the desktop link survives.
    Remove-Item -LiteralPath $icon
    Run-Setup '/S /DESKTOP=1'
    if ((Get-FileHash -LiteralPath $icon).Hash -cne (Get-FileHash -LiteralPath $packedIcon).Hash) { throw 'Reinstall did not restore the shortcut artwork.' }
    if ((Get-FileHash -LiteralPath $shortcuts[0]).Hash -cne $customHash) { throw 'An enabled shortcut lost its user customization.' }
    Run-Setup '/S'
    Run-Setup '/S /STARTUP=1'
    if ((Get-FileHash -LiteralPath $shortcuts[0]).Hash -cne $customHash) { throw 'A silent upgrade changed the existing desktop shortcut.' }
    Assert-Startup '' $false $statePath
    # A stale saved true value must still reconcile Windows when a resident engine handles the request.
    $testData = Join-Path $fixtureRoot 'test data'
    New-Item -ItemType Directory -Path $testData | Out-Null
    $testStatePath = Join-Path $testData 'state.json'
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    $state.settings.launch_at_login = $true
    $state.settings.show_on_launch = $false
    $state.settings.discord_presence = $false
    $state.base_visible = $false
    [IO.File]::WriteAllText($testStatePath, ($state | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
    $arguments = '--ipc-scope deployment-qa --test-data-dir "' + $testData + '" --no-input'
    # Start from the installed editor, as the Start menu does, to exercise a cold engine launch.
    $editorPid = [PackageActivation]::Start(($installed.PackageFamilyName + '!Editor'), $arguments)
    $engineStarted = $true
    $pipeName = '\\.\pipe\LOCAL\crosshair-rs.control.v1.' + [Diagnostics.Process]::GetCurrentProcess().SessionId + '.deployment-qa.' + $installed.PackageFamilyName
    Wait-ControlPipe
    $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    $sid = [PackageActivation]::WidgetSid($installed.PackageFamilyName)
    $widgetPipe = "\\.\pipe\Sessions\$session\AppContainerNamedObjects\$sid\crosshair-rs.widget.v1.$session"
    if (![PackageActivation]::WaitNamedPipe($widgetPipe, 1000)) { throw 'The installed engine could not open its Game Bar widget connection.' }
    # Windows startup is disabled even though the synthetic settings say enabled.
    $startupRead = '--ipc-scope deployment-qa --command "{\"type\":\"read_startup\"}"'
    $startup = (Activate-Request $startupRead | ConvertFrom-Json).result.Ok.status.startup_enabled
    if ($startup -isnot [bool] -or $startup) { throw 'The on-demand read did not report the actual disabled Windows startup task.' }
    Assert-Startup $arguments $true $testStatePath
    $startup = (Activate-Request $startupRead | ConvertFrom-Json).result.Ok.status.startup_enabled
    if ($startup -ne $true) { throw 'The on-demand read did not observe the enabled Windows startup task.' }
    Assert-Startup $arguments $false $testStatePath
    $saved = Get-Content -LiteralPath $testStatePath -Raw | ConvertFrom-Json
    if ($saved.settings.show_on_launch -or $saved.settings.discord_presence -or $saved.base_visible -or $saved.revision -le $state.revision) { throw 'Startup reconciliation changed unrelated settings or did not commit.' }
    $residents = @(Get-CimInstance Win32_Process -Filter "Name = 'crosshair-engine.exe'" | Where-Object {
        $_.ExecutablePath -ieq (Join-Path $installed.InstallLocation 'crosshair-engine.exe') -and
        $_.CommandLine -like '*--ipc-scope deployment-qa*' -and $_.CommandLine -notlike '*--configure-startup*'
    })
    if ($residents.Count -ne 1) { throw 'Startup configuration must leave one isolated engine running.' }
    $enginePid = $residents[0].ProcessId
    $editor = Get-Process -Id $editorPid -ErrorAction SilentlyContinue
    if (!$editor) { throw 'The isolated editor exited before its taskbar icon could be checked.' }
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while (!$editor.HasExited -and ![PackageActivation]::HasLargeIcon($editor.MainWindowHandle)) {
            if ([DateTime]::UtcNow -gt $deadline) { throw 'The isolated editor did not expose its large taskbar icon.' }
            Start-Sleep -Milliseconds 50
            $editor.Refresh()
        }
        if ($editor.HasExited -or !$editor.CloseMainWindow()) { throw 'The isolated editor window closed unexpectedly.' }
        if (!$editor.WaitForExit(15000)) { throw 'The isolated editor did not close.' }
    } finally { $editor.Dispose() }
    $markerPath = Join-Path $env:LOCALAPPDATA "Packages/$($installed.PackageFamilyName)/TempState/crosshair-widget-$session.open"
    $marker = [IO.FileStream]::new($markerPath, 'CreateNew', 'Write', 'None', 4096, [IO.FileOptions]::DeleteOnClose)
    try { Assert-WidgetWindow $true } finally { $marker.Dispose() }
    Assert-WidgetWindow $false
    Stop-TestEngine $enginePid
    $engineStarted = $false
    $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $installed.InstallLocation 'crosshair-engine.exe'), '--build-info')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $engine = [Diagnostics.Process]::Start($start)
    try {
        if (!$engine.WaitForExit(15000)) { $engine.Kill(); $engine.WaitForExit(); throw 'Installed engine did not start.' }
        $info = $engine.StandardOutput.ReadToEnd() | ConvertFrom-Json
        if ($engine.ExitCode -or $info.version -ne $build.version) { throw 'Installed engine version mismatch.' }
    } finally { $engine.Dispose() }
    Run-Setup '/S /DESKTOP=0 /STARTMENU=0'
    foreach ($shortcut in $shortcuts) { if (Test-Path -LiteralPath $shortcut) { throw 'Explicitly clearing a shortcut option did not remove its owned shortcut.' } }
    Assert-ShortcutDefaults 0
    Run-Setup '/S /DESKTOP=1'
    Assert-ShortcutDefaults 1
    Run-Setup '/S /DESKTOP=0'
    $unrelatedTarget = Join-Path $fixtureRoot 'unrelated.txt'
    Set-Content -LiteralPath $unrelatedTarget -Value 'Unrelated shortcut target'
    $links = New-Object -ComObject WScript.Shell
    foreach ($shortcut in $shortcuts) {
        $link = $links.CreateShortcut($shortcut)
        $link.TargetPath = $unrelatedTarget
        $link.Save()
    }
    Assert-ShortcutDefaults 0
    $unrelatedHashes = @($shortcuts | ForEach-Object { (Get-FileHash -LiteralPath $_).Hash })
    Run-Setup '/S /DESKTOP=0 /STARTMENU=0'
    for ($index = 0; $index -lt $shortcuts.Count; $index++) {
        if ((Get-FileHash -LiteralPath $shortcuts[$index]).Hash -cne $unrelatedHashes[$index]) { throw 'Setup changed an unrelated shortcut with the same name.' }
    }
    # Exercise the real upgrade installer with an older package identity version and synthetic settings.
    $previousVersion = '0.0.0.1'
    if ([version]$build.packageVersion -gt [version]$previousVersion) {
        $layout = Join-Path $fixtureRoot 'previous-layout'
        $previous = Join-Path $fixtureRoot 'previous.msix'
        $makeappx = Get-WindowsSdkTool 'makeappx'
        & $makeappx unpack /p (Join-Path $ArtifactDirectory $build.package) /d $layout | Out-Null
        if ($LASTEXITCODE) { throw 'Could not unpack the upgrade fixture.' }
        $manifestPath = Join-Path $layout 'AppxManifest.xml'
        [xml]$manifest = Get-Content -Raw -LiteralPath $manifestPath
        $manifest.Package.Identity.SetAttribute('Version', $previousVersion)
        $manifest.Save($manifestPath)
        & $makeappx pack /d $layout /p $previous | Out-Null
        if ($LASTEXITCODE) { throw 'Could not pack the previous-version fixture.' }
        Remove-AppxPackage -Package $installed.PackageFullName
        Copy-Item -LiteralPath $ownedDesktop -Destination $shortcuts[0] -Force
        Assert-ShortcutDefaults 1
        Add-AppxPackage -AllowUnsigned -Path $previous
        $old = Get-AppxPackage -Name $identity.Name
        if ($old.Version -ne $previousVersion -or $old.PackageFamilyName -cne $installed.PackageFamilyName) { throw 'Wrong upgrade fixture registration.' }
        New-Item -ItemType Directory -Force (Split-Path -Parent $statePath) | Out-Null
        Copy-Item -LiteralPath $testStatePath -Destination $statePath
        $stateHash = (Get-FileHash -LiteralPath $statePath).Hash
        Run-Setup '/S'
        $upgraded = Get-AppxPackage -Name $identity.Name
        if ($upgraded.Version -ne $build.packageVersion -or $upgraded.Status -ne 'Ok' -or
            (Get-FileHash -LiteralPath $statePath).Hash -cne $stateHash) { throw 'The in-place upgrade failed to retain the saved settings.' }
        Write-Output 'The normal setup upgraded an older package in place and preserved the saved settings byte-for-byte.'
    }
    # Exercise the ordinary packaged startup path using the disposable runner's own package storage.
    $beforeLaunch = Get-BrokerCount
    $enginePid = [PackageActivation]::Start($engineApp, '--ipc-scope deployment-qa')
    $engineStarted = $true
    Wait-ControlPipe
    $status = (Activate-Request '--ipc-scope deployment-qa --command "{\"type\":\"subscribe\"}"' | ConvertFrom-Json).result.Ok.status
    if (!$status.PSObject.Properties['startup_enabled'] -or $null -ne $status.startup_enabled) { throw 'Desktop launch read Windows startup settings before they were requested.' }
    $onDesktop = Get-BrokerCount
    $status = (Activate-Request $startupRead | ConvertFrom-Json).result.Ok.status
    if ($status.startup_enabled -isnot [bool]) { throw 'The installed app did not return the Windows sign-in setting.' }
    Write-Output "Package Runtime Broker instances before launch: $beforeLaunch, on Desktop: $onDesktop, after reading sign-in: $(Get-BrokerCount)."
    Stop-TestEngine $enginePid
    $engineStarted = $false
    Write-Output 'Setup registration, shortcut defaults and ownership, silent-option preservation and startup reconciliation after a cold editor launch passed. Windows sign-in itself was not exercised.'
} finally {
    if ($engineStarted) {
        try { Activate-Request '--ipc-scope deployment-qa --command "{\"type\":\"quit\",\"discard_unsaved\":false}"' | Out-Null } catch { Write-Warning 'The isolated engine did not acknowledge cleanup before package removal.' }
    }
    $installed = Get-AppxPackage -Name $identity.Name
    if ($installed -and $installed.Publisher -ceq $identity.Publisher) { Remove-AppxPackage -Package $installed.PackageFullName }
    foreach ($shortcut in $shortcuts) { if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut } }
    $artifactRoot = [IO.Path]::GetFullPath($ArtifactDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if (!$fixtureRoot.StartsWith($artifactRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe deployment fixture cleanup path.' }
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
}
