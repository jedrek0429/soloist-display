# Soloist Display Windows x86_64 bootstrap (External Spotify Soloist only).
# This release-rendered script embeds an immutable binary SHA-256.
[CmdletBinding()]
param(
    [switch]$Yes,
    [switch]$NoColor,
    [switch]$Quiet,
    [switch]$VerboseOutput
)
$ErrorActionPreference = 'Stop'
$releaseBase = '@SOLOIST_DISPLAY_RELEASE_BASE@'
$expectedSha = '@SOLOIST_DISPLAY_SHA_WINDOWS_X86_64@'
if ($releaseBase -notmatch '^https://github\.com/jedrek0429/soloist-display/releases/download/v') {
    throw 'This is an unrendered bootstrap. Download install.ps1 from a published release.'
}
if ($expectedSha -notmatch '^[0-9a-f]{64}$') {
    throw 'This release has no signed Windows binary. Use an alpha/beta release with desktop support.'
}
$tag = ($releaseBase -split '/')[-1]
$requested = $env:SOLOIST_DISPLAY_VERSION
if ($requested -and $requested -ne 'latest' -and "v$requested" -ne $tag) {
    throw "To install $requested, download its own versioned install.ps1 (hashes are release-specific)."
}
if (-not [Environment]::Is64BitOperatingSystem -or -not [Environment]::Is64BitProcess) {
    throw 'Windows x86_64 and 64-bit PowerShell are required.'
}
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('soloist-display-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $binary = Join-Path $tempRoot 'soloist-display.exe'
    $url = "$releaseBase/soloist-display-windows-x86_64.exe"
    if (-not $Quiet) { Write-Host "Downloading $tag (Windows x86_64)" }
    Invoke-WebRequest -Uri $url -OutFile $binary -MaximumRedirection 5
    $actualSha = (Get-FileHash -Path $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha -ne $expectedSha) {
        throw "Soloist Display SHA-256 mismatch: expected $expectedSha, got $actualSha"
    }
    $flags = @('install')
    if ($Yes) { $flags += '--yes' }
    if ($NoColor) { $flags += '--no-color' }
    if ($Quiet) { $flags += '--quiet' }
    if ($VerboseOutput) { $flags += '--verbose' }
    $env:SOLOIST_DISPLAY_VERSION = $tag.Substring(1)
    & $binary @flags
    if ($LASTEXITCODE -ne 0) { throw "Soloist Display installer exited with code $LASTEXITCODE" }

    $binDir = if ($env:SOLOIST_DISPLAY_BIN_DIR) {
        $env:SOLOIST_DISPLAY_BIN_DIR
    } elseif ($env:SOLOIST_DISPLAY_INSTALL_ROOT) {
        Join-Path $env:SOLOIST_DISPLAY_INSTALL_ROOT 'bin'
    } else {
        Join-Path $env:LOCALAPPDATA 'soloist-display\bin'
    }
    if (-not [IO.Path]::IsPathRooted($binDir)) { throw "Invalid CLI bin path: $binDir" }
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @($userPath -split ';' | Where-Object { $_ })
    if (-not ($parts | Where-Object { $_.TrimEnd('\') -ieq $binDir.TrimEnd('\') })) {
        $updated = (($parts + $binDir) -join ';')
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
        if (-not $Quiet) { Write-Host "Added $binDir to your user PATH. Open a new terminal to use soloist-display." }
    }
} finally {
    Remove-Item -Path $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
