# Builds UmlCraft as the release workflow does and installs it for the current user with the
# Windows installer, so it starts from the Start menu like a downloaded release:
#
#   pwsh -File tools\install-local.ps1
#
# Needs Inno Setup 7 (ISCC.exe). Without a THIRD-PARTY-LICENSES.html from the release workflow
# (`cargo about`), the installer gets a placeholder. A running installed UmlCraft is closed.

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$iscc = @(
    "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 7 (ISCC.exe) is not installed.' }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value

cargo build --release --locked -p umlcraft-app -p umlcraft-cli
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$stage = Join-Path $root 'target\local-install'
$source = Join-Path $stage 'files'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $source | Out-Null
Copy-Item target\release\umlcraft.exe, target\release\umlcraft-cli.exe, README.md,
    LICENSE-MIT, LICENSE-APACHE $source
if (Test-Path THIRD-PARTY-LICENSES.html) {
    Copy-Item THIRD-PARTY-LICENSES.html $source
} else {
    Set-Content -Path (Join-Path $source 'THIRD-PARTY-LICENSES.html') -Encoding utf8 `
        -Value '<p>Local build: the licences of the dependencies are listed in the releases.</p>'
}

& $iscc /Qp "/DVersion=$version" "/DSource=$source" "/O$stage" packaging\windows\umlcraft.iss
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$setup = Join-Path $stage "UmlCraft-$version-windows-x64-setup.exe"
$installer = Start-Process $setup -Wait -PassThru -ArgumentList '/VERYSILENT',
    '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER', '/CLOSEAPPLICATIONS'
if ($installer.ExitCode) { throw "The installer failed with exit code $($installer.ExitCode)." }
Write-Output "UmlCraft $version installed: $env:LOCALAPPDATA\Programs\UmlCraft\umlcraft.exe"
