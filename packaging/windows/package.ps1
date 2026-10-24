<#
.SYNOPSIS
  Build, sign and package Mortiflix Music for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    mortiflix-music-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    mortiflix-music-<version>-windows-<arch>-portable.zip   mortiflix-music.exe + mortiflix-music-cli.exe + portable.txt

  Mortiflix Music installs beside an installed Mortiflix Studio without clashing: its own product,
  UpgradeCode, folder (Program Files\Mortiflix Music), Start Menu entry and exe names, and no file
  associations. cargo still builds mortiflix-studio(-cli).exe; they are renamed here, and the app
  starts as Mortiflix Music because its exe name starts with mortiflix-music.

  The binaries link the C runtime statically (+crt-static), so neither the MSI nor the portable
  zip needs the Visual C++ redistributable. Signing is delegated to sign.ps1 (skipped with a
  warning when no signing secrets are set).

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (rc.exe, signtool.exe),
  and WiX v5: dotnet tool install --global wix --version 5.0.2
  (+ wix extension add -g WixToolset.UI.wixext/5.0.2 for the installer's wizard)

  Mortiflix Music: both also carry the Mortiflix bundle (the service, its own Node, ffmpeg,
  Strudel, the GeneralUser GS soundfont, FluidSynth) in mortiflix\ beside mortiflix-music.exe, made
  by packaging/mortiflix/bundle.sh under Git Bash (env: MFX_SERVICE_SRC, MFX_SERVICE_REF, NODE_VERSION,
  ...). -NoMortiflix leaves it out. There is no tools step: nothing is downloaded at install time.

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x86 -SkipBuild
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild,
  [switch] $NoMortiflix
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:MFX_STUDIO_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $env:MFX_STUDIO_BUILD_SHA) { $env:MFX_STUDIO_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:MFX_STUDIO_BUILD_DATE) { $env:MFX_STUDIO_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }

Write-Output "Mortiflix Music $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  # Fail the build (rather than warn) if the icon/VERSIONINFO can't be embedded.
  $env:MFX_STUDIO_REQUIRE_WINRES = '1'
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p mortiflix-studio -p mortiflix-studio-cli --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"

# Check both binaries' PE headers before packaging. Machine (COFF header) must match -Arch, so an
# x64 build can never ship labelled arm64. Subsystem (optional header): 2 = Windows GUI, 3 = console.
# The app must be GUI (no console window opens with it); the CLI must stay console so its output
# reaches the terminal.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('mortiflix-studio.exe', 2), @('mortiflix-studio-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
# Installed under Mortiflix Music's own names: the app picks the music product from its exe name
# (crates/mortiflix-ui/src/lib.rs `product()`), and nothing clashes with an installed Mortiflix Studio.
Copy-Item (Join-Path $Bin 'mortiflix-studio.exe') (Join-Path $Stage 'mortiflix-music.exe')
Copy-Item (Join-Path $Bin 'mortiflix-studio-cli.exe') (Join-Path $Stage 'mortiflix-music-cli.exe')

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'mortiflix-music.exe') (Join-Path $Stage 'mortiflix-music-cli.exe')

# The licence page of the wizard: the MIT licence, and where the bundle's licences are.
$LicenseRtf = Join-Path $TargetDir "windows-package\license-$Arch.rtf"
$mit = (Get-Content -Raw (Join-Path $Root 'LICENSE-MIT')) -replace '\\', '\\' -replace '\{', '\{' -replace '\}', '\}' -replace "`r?`n", "\par`n"
Set-Content -Encoding ascii -Path $LicenseRtf -Value ("{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs18 " + $mit +
  "\par\par The mortiflix\\ folder holds separate programs (Node, Strudel, FluidSynth, ffmpeg, the GeneralUser GS soundfont) with their own licences: see mortiflix\\LICENSES.}")

# ---- Mortiflix bundle ----------------------------------------------------------------------------
# Found by the app as mortiflix\ beside mortiflix-music.exe (crates/mortiflix-ui/src/service.rs).
$MfxDir = Join-Path $Stage 'mortiflix'
$WixMfx = @()
if (-not $NoMortiflix) {
  if ($Arch -eq 'x86') { throw 'the Mortiflix bundle needs x64 or arm64 (no 32-bit Node or ffmpeg builds); use -NoMortiflix' }
  # Git Bash (bash.exe on PATH can be WSL's, which can't see this checkout the same way).
  $GitBash = Join-Path $env:ProgramFiles 'Git\bin\bash.exe'
  $Bash = if ($env:BASH) { $env:BASH } elseif (Test-Path $GitBash) { $GitBash } else { 'bash' }
  Invoke-Native 'Mortiflix bundle' { & $Bash (Join-Path $Root 'packaging/mortiflix/bundle.sh') --os windows --arch $Arch --out ($MfxDir -replace '\\', '/') }
  $MfxExes = Get-ChildItem -Path $MfxDir -Recurse -Include '*.exe' | ForEach-Object { $_.FullName }
  if ($MfxExes) { & (Join-Path $PSScriptRoot 'sign.ps1') @MfxExes }
  $WixMfx = @('-d', "MfxDir=$MfxDir")
}

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "mortiflix-music-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'mortiflix-music.wxs') -arch $Arch `
    -d "Version=$MsiVersion" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\mortiflix-studio.ico')" `
    -d "LicenseRtf=$LicenseRtf" -ext WixToolset.UI.wixext @WixMfx -o $Msi
}
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\mortiflix-music-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
if (Test-Path $MfxDir) { Copy-Item -Recurse $MfxDir (Join-Path $Portable 'mortiflix') }
# The marker that makes Mortiflix Music keep its data in .\data next to the exe instead of %APPDATA% (#139).
Set-Content -Encoding ascii -Path (Join-Path $Portable 'portable.txt') -Value @(
  'Mortiflix Music portable: preferences, presets, logs and crash recovery are kept in the data folder',
  'next to mortiflix-music.exe. Delete this file to use %APPDATA%\Mortiflix Music instead.'
)
foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
# Licences of the embedded craft-fonts fonts (builds with CRAFT_FONTS_DIR) as OFL-<family>.txt.
if ($env:CRAFT_FONTS_DIR) {
  foreach ($lic in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -ErrorAction SilentlyContinue) {
    Copy-Item $lic.FullName (Join-Path $Portable "OFL-$($lic.Directory.Name).txt")
  }
}
$Zip = Join-Path $Dist "mortiflix-music-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# Smoke-test the CLI when this machine can run it. An ARM64 build made on an x64 runner can't run
# here; .github/workflows/windows-arm64.yml installs and runs it on ARM64 instead.
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -ne 'arm64' -or $HostArch -eq 'arm64') {
  Invoke-Native 'mortiflix-music-cli --version' { & (Join-Path $Stage 'mortiflix-music-cli.exe') --version }
} else {
  Write-Output "skipping mortiflix-music-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Msi, $Zip | Format-Table Name, Length
