<#
.SYNOPSIS
Install the toolr binary from a GitHub release on Windows.

.EXAMPLE
iwr -useb https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.ps1 | iex
#>
[CmdletBinding()]
param(
  [string]$Version,
  [string]$Triple = "x86_64-pc-windows-msvc",
  [string]$Prefix,
  [string]$Repo = "s0undt3ch/ToolR",
  [switch]$DryRun,
  [switch]$NoVerify
)

$ErrorActionPreference = "Stop"

function Resolve-LatestVersion {
  $resp = Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -ErrorAction SilentlyContinue `
    -Uri "https://github.com/$Repo/releases/latest"
  $loc = $resp.Headers["Location"]
  if (-not $loc) { throw "Could not resolve latest version" }
  $tag = ($loc -split "/tag/")[-1].TrimEnd('/')
  if ($tag -notmatch '^v(.+)$') { throw "Unexpected tag format: $tag" }
  return $matches[1]
}

if (-not $Version) { $Version = Resolve-LatestVersion }
if (-not $Prefix) {
  $localApp = Join-Path $env:LOCALAPPDATA "Programs\toolr"
  $Prefix = $localApp
}

$filename = "toolr-$Version-$Triple.zip"
$url = "https://github.com/$Repo/releases/download/v$Version/$filename"
$shaUrl = "$url.sha256"

Write-Host "version: $Version"
Write-Host "triple:  $Triple"
Write-Host "prefix:  $Prefix"
Write-Host "url:     $url"

if ($DryRun) { Write-Host "dry-run; exiting"; return }

$tmp = Join-Path $env:TEMP ("toolr-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  $zipPath = Join-Path $tmp $filename
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zipPath
  if (-not $NoVerify) {
    $shaFile = Join-Path $tmp "$filename.sha256"
    Invoke-WebRequest -UseBasicParsing -Uri $shaUrl -OutFile $shaFile
    $expected = (Get-Content $shaFile -Raw).Split(" ")[0].Trim().ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 $zipPath).Hash.ToLower()
    if ($expected -ne $actual) {
      throw "Checksum mismatch: expected $expected got $actual"
    }
  }
  Expand-Archive -Path $zipPath -DestinationPath $tmp
  $extracted = Join-Path $tmp "toolr-$Version-$Triple"
  if (-not (Test-Path $extracted)) { throw "Unexpected archive layout" }
  New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
  Copy-Item -Force (Join-Path $extracted "toolr.exe") (Join-Path $Prefix "toolr.exe")
  Write-Host "installed: $(Join-Path $Prefix 'toolr.exe')"
  if (-not (($env:Path -split ';') -contains $Prefix)) {
    Write-Host "note: $Prefix is not on \$env:Path; add it to your environment"
  }
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
