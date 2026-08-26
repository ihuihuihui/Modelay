param(
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$projectDir = Split-Path -Parent $PSScriptRoot
$bundleDir = Join-Path $projectDir "src-tauri\target\release\bundle"
$outputDir = Join-Path $projectDir "artifacts\installers"

Set-Location $projectDir
if (-not $SkipBuild) {
  npm run tauri build -- --bundles nsis,msi
  if ($LASTEXITCODE -ne 0) { throw "Tauri Windows build failed." }
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
$installers = @(
  Get-ChildItem (Join-Path $bundleDir "nsis\*.exe") -File -ErrorAction SilentlyContinue
  Get-ChildItem (Join-Path $bundleDir "msi\*.msi") -File -ErrorAction SilentlyContinue
)
if ($installers.Count -eq 0) {
  throw "No NSIS or MSI installers were found under $bundleDir."
}

$copied = foreach ($installer in $installers) {
  $destination = Join-Path $outputDir $installer.Name
  Copy-Item $installer.FullName $destination -Force
  Get-Item $destination
}

$checksumPath = Join-Path $outputDir "Modelay-Windows-SHA256.txt"
$lines = foreach ($installer in $copied) {
  $hash = (Get-FileHash -Algorithm SHA256 $installer.FullName).Hash.ToLowerInvariant()
  "$hash  $($installer.Name)"
}
Set-Content -Path $checksumPath -Value $lines -Encoding ascii

$copied.FullName
$checksumPath
