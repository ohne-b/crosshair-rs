$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$paths = @((Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -File).FullName) + @((Get-ChildItem -LiteralPath (Join-Path $repoRoot 'packaging/windows') -Filter '*.ps1' -File).FullName)
$paths += Join-Path $repoRoot 'apps/engine/src/updates/deploy.ps1'
$snippets = Join-Path $repoRoot 'target/workflow-scripts'
if (Test-Path -LiteralPath $snippets) { $paths += @((Get-ChildItem -LiteralPath $snippets -Filter '*.ps1' -File).FullName) }
foreach ($path in ($paths | Where-Object { $_ })) {
    $tokens = $null; $errors = $null
    [void][Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "$([IO.Path]::GetFileName($path)): $($errors.Message -join '; ')" }
}
foreach ($path in @('packaging/windows/AppxManifest.xml', 'apps/gamebar/CrosshairWidget/CrosshairWidget.vcxproj', 'assets/logo.svg')) {
    [xml]$document = Get-Content -LiteralPath (Join-Path $repoRoot $path) -Raw
    if (!$document.DocumentElement) { throw "Empty XML: $path" }
}
Write-Output 'PowerShell and package XML parse checks passed.'
