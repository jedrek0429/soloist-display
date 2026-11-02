param(
    [switch]$Yes,
    [switch]$NoColor,
    [switch]$Quiet,
    [switch]$Verbose,
    [switch]$Help,
    [switch]$Version,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Remaining
)

$ErrorActionPreference = 'Stop'
$bootstrapVersion = 1
$pinnedBase = '@SOLOIST_DISPLAY_RELEASE_BASE@'
$pinnedX64 = '@SOLOIST_DISPLAY_SHA_WINDOWS_X86_64@'
$pinnedArm64 = '@SOLOIST_DISPLAY_SHA_WINDOWS_ARM64@'

if ($Help) {
    @'
Usage: install.ps1 [OPTIONS]

Downloads and verifies the Soloist Display installer, then runs it.

  -Yes          accept safe defaults
  -NoColor      disable coloured output
  -Quiet        show only errors
  -Verbose      show additional diagnostics
  -Help         show this help
  -Version      show bootstrap version
'@
    exit 0
}
if ($Version) { Write-Output "soloist-display-bootstrap $bootstrapVersion"; exit 0 }
if (-not $IsWindows -and $PSVersionTable.PSEdition -eq 'Core') { throw 'Windows is required for install.ps1' }

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($architecture) {
    'X64' { $target = 'x86_64-pc-windows-msvc'; $expected = $pinnedX64 }
    'Arm64' { $target = 'aarch64-pc-windows-msvc'; $expected = $pinnedArm64 }
    default { throw "Unsupported Windows architecture: $architecture" }
}
if ($expected -eq 'unavailable' -or $expected.StartsWith('@')) {
    throw "No validated Soloist Display release is published for $target"
}

$requested = if ($env:SOLOIST_DISPLAY_VERSION) { $env:SOLOIST_DISPLAY_VERSION } else { 'latest' }
$releaseBase = if ($requested -eq 'latest') {
    if ($pinnedBase -notlike '@*') { $pinnedBase } else { 'https://github.com/jedrek0429/soloist-display/releases/latest/download' }
} elseif ($requested -match '^\d+\.\d+\.\d+$') {
    "https://github.com/jedrek0429/soloist-display/releases/download/v$requested"
} else { throw 'SOLOIST_DISPLAY_VERSION must be X.Y.Z or latest' }
if ($pinnedBase -notlike '@*' -and $releaseBase -ne $pinnedBase) {
    throw 'Use the install.ps1 asset from the requested signed release so its installer digest is pinned'
}

$temporary = Join-Path ([System.IO.Path]::GetTempPath()) "soloist-display-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $installer = Join-Path $temporary 'soloist-display.exe'
    $url = "$releaseBase/soloist-display-$target.exe"
    if (-not $Quiet) { Write-Host "  Downloading $requested · $architecture" }
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $installer
    $actual = (Get-FileHash -Algorithm SHA256 -Path $installer).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) { throw 'Installer SHA-256 verification failed' }
    $arguments = @('install')
    if ($Yes) { $arguments += '--yes' }
    if ($NoColor) { $arguments += '--no-color' }
    if ($Quiet) { $arguments += '--quiet' }
    if ($Verbose) { $arguments += '--verbose' }
    $arguments += $Remaining
    & $installer @arguments
    if ($LASTEXITCODE -ne 0) { throw "Installer exited with code $LASTEXITCODE" }
} finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
