# Embedded by the update helper; Package, ExpectedHash and ExpectedUser are quoted literals.
$ErrorActionPreference = 'Stop'
$directory = $null
$locked = $null
$exitCode = 0
try {
    if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ExpectedUser) { throw 'Use the same Windows account to approve the update.' }
    $administrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (!$administrator.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Unsigned MSIX updates require administrator approval.' }
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    $directory = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) ('crosshair-rs-update-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory, $acl) | Out-Null
    $copy = Join-Path $directory 'package.msix'
    [IO.File]::Copy($Package, $copy, $false)
    $locked = [IO.File]::Open($copy, 'Open', 'Read', 'Read')
    if ((Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedHash) { throw 'The verified package changed before deployment.' }
    # The elevated operation consumes only this verified copy in an administrator-owned directory.
    Add-AppxPackage -Path $copy -AllowUnsigned -ErrorAction Stop
} catch {
    $exitCode = $_.Exception.HResult
    if (!$exitCode) { $exitCode = 1 }
    Write-Error -Message $_ -ErrorAction Continue
} finally {
    if ($locked) { $locked.Dispose() }
    if ($directory) {
        $copy = Join-Path $directory 'package.msix'
        if (Test-Path -LiteralPath $copy) { Remove-Item -LiteralPath $copy -ErrorAction SilentlyContinue }
        Remove-Item -LiteralPath $directory -ErrorAction SilentlyContinue
    }
}
exit $exitCode
