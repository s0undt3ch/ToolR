<#
.SYNOPSIS
Install the toolr binary from a GitHub release on Windows.

.EXAMPLE
iwr -useb https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
#>
[CmdletBinding()]
param(
  [string]$Version,
  [string]$Triple = "x86_64-pc-windows-msvc",
  [string]$Prefix,
  [string]$Repo = "s0undt3ch/ToolR",
  [switch]$DryRun,
  [switch]$NoVerify,
  [ValidateSet("auto", "require", "skip")]
  [string]$VerifyAttestation = "require"
)

$ErrorActionPreference = "Stop"

function Resolve-LatestVersion {
  # The /releases/latest endpoint returns a 302 redirect to the
  # actual tag page. PowerShell 7's Invoke-WebRequest treats the
  # 302 as an error and throws even with `-ErrorAction
  # SilentlyContinue` when `-MaximumRedirection 0` is set, so we
  # ask the GitHub REST API instead — it returns the tag name in
  # JSON with no redirect dance. Auth is optional but honoured
  # when GH_TOKEN is set so the request doesn't hit unauthenticated
  # rate limits in CI smoke runs.
  $headers = @{ "Accept" = "application/vnd.github+json" }
  if ($env:GH_TOKEN) {
    $headers["Authorization"] = "Bearer $env:GH_TOKEN"
  }
  $resp = Invoke-RestMethod -UseBasicParsing -Headers $headers `
    -Uri "https://api.github.com/repos/$Repo/releases/latest"
  if (-not $resp.tag_name) { throw "Could not resolve latest version (no tag_name in API response)" }
  $tag = $resp.tag_name
  if ($tag -notmatch '^v(.+)$') { throw "Unexpected tag format: $tag" }
  return $matches[1]
}

function Test-AttestationVerified {
  param([string]$Path)
  if ($VerifyAttestation -eq "skip") {
    Write-Host "skipping attestation verification (-VerifyAttestation skip)"
    return
  }
  $gh = Get-Command gh -ErrorAction SilentlyContinue
  if (-not $gh) {
    if ($VerifyAttestation -eq "require") {
      throw @"
cannot verify the release's SLSA build provenance: the 'gh' CLI is not on PATH.
  toolr archives are signed; verification needs GitHub CLI -> https://cli.github.com
  Choose one:
    * install 'gh' and re-run this installer (recommended), or
    * re-run with -VerifyAttestation skip to install WITHOUT supply-chain verification.
"@
    }
    Write-Warning "skipping attestation verification ('gh' CLI not installed) -- the archive is NOT supply-chain verified"
    return
  }
  # gh before 2.56 can't load GitHub's current Sigstore trusted root, so
  # an old gh fails in `auto` too rather than skipping silently.
  $ghVersion = (& gh --version | Select-Object -First 1) -as [string]
  if (-not ($ghVersion -match '^gh version (\d+)\.(\d+)') -or
      [int]$matches[1] -lt 2 -or ([int]$matches[1] -eq 2 -and [int]$matches[2] -lt 56)) {
    throw @"
gh >= 2.56 is required to verify attestations (found: $ghVersion)
  upgrade 'gh', or re-run with -VerifyAttestation skip to install WITHOUT supply-chain verification.
"@
  }
  # Same rules as action.yml's verify-attestation step: ci.yml shares the signer, so only
  # buildConfigURI tells a release from a CI build; one subject must carry this name and digest.
  # The filter stays on one line with no quotes for Windows PowerShell's native argument passing.
  $signerWorkflow = "https://github.com/$Repo/.github/workflows/_build-binary-archive.yml@refs/heads/main"
  $releaseWorkflow = "https://github.com/$Repo/.github/workflows/release.yml@refs/heads/main"
  $archiveName = Split-Path -Leaf $Path
  $jqFilter = '[.[].verificationResult | select(.signature.certificate.buildConfigURI == env.RELEASE_WORKFLOW and any(.statement.subject[]; .name == env.ARCHIVE_NAME and .digest.sha256 == env.ARCHIVE_SHA256))] | length'
  $archiveSha256 = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLower()
  $savedEnv = @{
    RELEASE_WORKFLOW = $env:RELEASE_WORKFLOW
    ARCHIVE_NAME = $env:ARCHIVE_NAME
    ARCHIVE_SHA256 = $env:ARCHIVE_SHA256
  }
  Write-Host "verifying SLSA build provenance via 'gh attestation verify'"
  try {
    $env:RELEASE_WORKFLOW = $releaseWorkflow
    $env:ARCHIVE_NAME = $archiveName
    $env:ARCHIVE_SHA256 = $archiveSha256
    $count = & gh attestation verify $Path --repo $Repo --cert-identity $signerWorkflow `
      --deny-self-hosted-runners --format json --jq $jqFilter
    $ghExit = $LASTEXITCODE
  } finally {
    $env:RELEASE_WORKFLOW = $savedEnv.RELEASE_WORKFLOW
    $env:ARCHIVE_NAME = $savedEnv.ARCHIVE_NAME
    $env:ARCHIVE_SHA256 = $savedEnv.ARCHIVE_SHA256
  }
  if ($ghExit -ne 0) {
    throw "attestation verification failed for $Path"
  }
  $parsed = 0
  if (-not ([int]::TryParse((($count | Out-String).Trim()), [ref]$parsed)) -or $parsed -lt 1) {
    throw @"
$archiveName has a valid $Repo attestation, but none from the $Repo release workflow on main for this exact archive
  install a published release; -VerifyAttestation skip turns off this check entirely.
"@
  }
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
  Test-AttestationVerified -Path $zipPath
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
