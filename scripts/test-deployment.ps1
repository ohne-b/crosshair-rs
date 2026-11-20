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
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool WaitNamedPipe(string name, uint timeout);
}
'@
$fixtureRoot = [IO.Path]::GetFullPath((Join-Path $ArtifactDirectory ('deployment-' + [Guid]::NewGuid().ToString('N'))))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$shortcutName = if ($identity.Name.EndsWith('.Preview')) { 'crosshair-rs Preview.lnk' } else { 'crosshair-rs.lnk' }
$shortcuts = @([Environment]::GetFolderPath('DesktopDirectory'), [Environment]::GetFolderPath('Programs')) | ForEach-Object { Join-Path $_ $shortcutName }
foreach ($shortcut in $shortcuts) { if (Test-Path -LiteralPath $shortcut) { throw 'A shortcut already exists; refusing to replace it.' } }
$setup = (Resolve-Path -LiteralPath (Join-Path $ArtifactDirectory $build.setup)).Path
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
$installed = $null
$engineStarted = $false
try {
    Run-Setup '/S /DESKTOP=1 /STARTMENU=1 /STARTUP=1'
    $installed = Get-AppxPackage -Name $identity.Name
    if (!$installed -or $installed.Version -ne $build.packageVersion -or $installed.Publisher -cne $identity.Publisher -or $installed.Status -ne 'Ok') { throw 'Windows did not register the expected package.' }
    $engineApp = $installed.PackageFamilyName + '!Engine'
    $statePath = Join-Path $env:LOCALAPPDATA ('Packages/' + $installed.PackageFamilyName + '/LocalState/state.json')
    if (!(Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json).settings.launch_at_login) { throw 'The installer startup option was not saved.' }
    if (Get-ChildItem -LiteralPath (Split-Path -Parent $statePath) -Filter 'startup-*.txt') { throw 'Setup left its startup reply behind.' }
    $shell = New-Object -ComObject Shell.Application
    foreach ($shortcut in $shortcuts) {
        if (!(Test-Path -LiteralPath $shortcut)) { throw 'An enabled installer shortcut is missing.' }
        $item = $shell.NameSpace((Split-Path -Parent $shortcut)).ParseName((Split-Path -Leaf $shortcut))
        if ($item.ExtendedProperty('System.AppUserModel.ID') -cne ($installed.PackageFamilyName + '!Editor')) { throw 'Shortcut does not use the stable packaged app identity.' }
    }
    Run-Setup '/S'
    Run-Setup '/S /STARTUP=1'
    foreach ($shortcut in $shortcuts) { if (!(Test-Path -LiteralPath $shortcut)) { throw 'A silent upgrade removed an omitted shortcut option.' } }
    Assert-Startup '' $false $statePath
    # A stale saved true value must still reconcile Windows when a resident engine handles the request.
    $testData = Join-Path $fixtureRoot 'data'
    New-Item -ItemType Directory -Path $testData | Out-Null
    $testStatePath = Join-Path $testData 'state.json'
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    $state.settings.launch_at_login = $true
    $state.settings.show_on_launch = $false
    $state.settings.discord_presence = $false
    $state.base_visible = $false
    [IO.File]::WriteAllText($testStatePath, ($state | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
    $arguments = '--ipc-scope deployment-qa --test-data-dir "' + $testData + '" --no-input'
    $enginePid = [PackageActivation]::Start($engineApp, $arguments)
    $engineStarted = $true
    $pipeName = '\\.\pipe\LOCAL\crosshair-rs.control.v1.' + [Diagnostics.Process]::GetCurrentProcess().SessionId + '.deployment-qa'
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (![PackageActivation]::WaitNamedPipe($pipeName, 100)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The isolated engine did not open its IPC endpoint.' }
        Start-Sleep -Milliseconds 50
    }
    Assert-Startup $arguments $true $testStatePath
    Assert-Startup $arguments $false $testStatePath
    $saved = Get-Content -LiteralPath $testStatePath -Raw | ConvertFrom-Json
    if ($saved.settings.show_on_launch -or $saved.settings.discord_presence -or $saved.base_visible -or $saved.revision -le $state.revision) { throw 'Startup reconciliation changed unrelated settings or did not commit.' }
    if (!(Get-Process -Id $enginePid -ErrorAction SilentlyContinue)) { throw 'Configuring startup stopped the resident engine.' }
    $quit = Activate-Request '--ipc-scope deployment-qa --command "{\"type\":\"quit\",\"discard_unsaved\":false}"'
    $response = $quit | ConvertFrom-Json
    if ($null -eq $response.result.PSObject.Properties['Ok']) { throw 'The isolated engine could not shut down.' }
    $resident = Get-Process -Id $enginePid -ErrorAction SilentlyContinue
    if ($resident) {
        try { if (!$resident.WaitForExit(15000)) { throw 'The isolated engine did not exit after acknowledging Quit.' } }
        finally { $resident.Dispose() }
    }
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
    $unrelatedTarget = Join-Path $fixtureRoot 'unrelated.txt'
    Set-Content -LiteralPath $unrelatedTarget -Value 'Unrelated shortcut target'
    $links = New-Object -ComObject WScript.Shell
    foreach ($shortcut in $shortcuts) {
        $link = $links.CreateShortcut($shortcut)
        $link.TargetPath = $unrelatedTarget
        $link.Save()
    }
    $unrelatedHashes = @($shortcuts | ForEach-Object { (Get-FileHash -LiteralPath $_).Hash })
    Run-Setup '/S /DESKTOP=0 /STARTMENU=0'
    for ($index = 0; $index -lt $shortcuts.Count; $index++) {
        if ((Get-FileHash -LiteralPath $shortcuts[$index]).Hash -cne $unrelatedHashes[$index]) { throw 'Setup changed an unrelated shortcut with the same name.' }
    }
    Write-Output 'Setup registration, stable shortcuts, silent-option preservation, packaged startup enable/disable and resident-engine reconciliation passed. Windows sign-in itself was not exercised.'
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
