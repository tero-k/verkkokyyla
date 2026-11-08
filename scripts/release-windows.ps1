[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^v\d+\.\d+\.\d+$')]
    [string]$Tag
)

$ErrorActionPreference = 'Stop'

function Invoke-Native {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE."
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'Windows release binaries must be built on Windows.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$windowsConfigPath = Join-Path $repoRoot 'src-tauri\tauri.windows.release.conf.json'
$signCommand = (Get-Content -Raw -LiteralPath $windowsConfigPath | ConvertFrom-Json).bundle.windows.signCommand
$signToolPath = $signCommand.cmd

if (-not (Test-Path -LiteralPath $signToolPath -PathType Leaf)) {
    throw "SignTool was not found at $signToolPath."
}

Push-Location $repoRoot
try {
    $branch = (Invoke-Native -Command 'git' -Arguments @('branch', '--show-current') | Select-Object -Last 1).Trim()
    if ($branch -ne 'main') {
        throw "Releases must be built from main; current branch is $branch."
    }

    $dirty = @(Invoke-Native -Command 'git' -Arguments @('status', '--porcelain'))
    if ($dirty.Count -ne 0) {
        throw 'Releases require a clean working tree.'
    }

    $headCommit = (Invoke-Native -Command 'git' -Arguments @('rev-parse', 'HEAD') | Select-Object -Last 1).Trim()
    $tagCommit = (Invoke-Native -Command 'git' -Arguments @('rev-parse', "$Tag^{}") | Select-Object -Last 1).Trim()
    if ($headCommit -ne $tagCommit) {
        throw "$Tag does not point at the current main commit."
    }

    $version = $Tag.Substring(1)
    $tauriConfig = Get-Content -Raw -LiteralPath 'src-tauri\tauri.conf.json' | ConvertFrom-Json
    $packageJson = Get-Content -Raw -LiteralPath 'package.json' | ConvertFrom-Json
    $cargoManifest = Get-Content -Raw -LiteralPath 'src-tauri\Cargo.toml'
    $cargoName = [regex]::Match($cargoManifest, '(?m)^name\s*=\s*"([^"]+)"\s*$').Groups[1].Value
    $cargoVersion = [regex]::Match($cargoManifest, '(?m)^version\s*=\s*"([^"]+)"\s*$').Groups[1].Value

    if ($tauriConfig.version -ne $version -or $packageJson.version -ne $version -or $cargoVersion -ne $version) {
        throw "Tag $Tag does not match all three project version fields ($($tauriConfig.version), $($packageJson.version), $cargoVersion)."
    }

    $isDraft = (Invoke-Native -Command 'gh' -Arguments @('release', 'view', $Tag, '--json', 'isDraft', '--jq', '.isDraft') | Select-Object -Last 1).Trim()
    if ($isDraft -ne 'true') {
        throw "Release $Tag must exist as a draft before Windows assets are built."
    }

    # The release-only config signs the main executable before packaging and
    # signs both installers afterward, using the local Certum certificate.
    # Disable per-bundle binary patching so it cannot invalidate that signature;
    # this project has no updater requiring bundle-type patching.
    Invoke-Native -Command 'npm' -Arguments @('run', 'tauri', '--', 'build', '--config', $windowsConfigPath, '--no-binary-patching')

    $productName = $tauriConfig.productName
    $binaryName = if ($tauriConfig.mainBinaryName) { $tauriConfig.mainBinaryName } else { $cargoName }
    $releaseDir = Join-Path $repoRoot 'src-tauri\target\release'
    $mainBinary = Join-Path $releaseDir "$binaryName.exe"
    $nsisInstaller = Join-Path $releaseDir "bundle\nsis\${productName}_${version}_x64-setup.exe"
    $msiInstaller = Join-Path $releaseDir "bundle\msi\${productName}_${version}_x64_en-US.msi"
    $signTargets = @($mainBinary, $nsisInstaller, $msiInstaller)

    foreach ($target in $signTargets) {
        if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
            throw "Expected release artifact was not created: $target"
        }
    }

    # Tauri can change the top-level executable again after its per-bundle
    # signing hook. Re-sign the final output before packaging the standalone ZIP.
    $mainSignArguments = @($signCommand.args | ForEach-Object { $_.Replace('%1', $mainBinary) })
    Invoke-Native -Command $signToolPath -Arguments $mainSignArguments

    foreach ($target in $signTargets) {
        Invoke-Native -Command $signToolPath -Arguments @('verify', '/pa', '/v', $target)
    }

    # The standalone ZIP contains the same signed main executable verified above.
    $standaloneZip = Join-Path $releaseDir "${productName}_${version}_x64-standalone.zip"
    Compress-Archive -LiteralPath $mainBinary -DestinationPath $standaloneZip -Force

    $assets = @($nsisInstaller, $msiInstaller, $standaloneZip)
    Invoke-Native -Command 'gh' -Arguments (@('release', 'upload', $Tag) + $assets + @('--clobber'))
    Invoke-Native -Command 'gh' -Arguments @('release', 'edit', $Tag, '--draft=false')

    $releaseUrl = (Invoke-Native -Command 'gh' -Arguments @('release', 'view', $Tag, '--json', 'url', '--jq', '.url') | Select-Object -Last 1).Trim()
    Write-Host "Published $releaseUrl"
}
finally {
    Pop-Location
}
