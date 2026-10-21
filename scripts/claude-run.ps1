# Written by Codex Cockpit.
# Usage:  powershell -NoProfile -ExecutionPolicy Bypass -File claude-run.ps1 [claude options] "prompt"
#    or:  Get-Content task.md -Raw | powershell -NoProfile -ExecutionPolicy Bypass -File claude-run.ps1 --stdin [claude options]
# Standard input is only read with --stdin, so an open but idle pipe can never stall a run.
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

# This script lives in <owner home>\.claude\cockpit. Sandboxed callers (such as
# Codex's Windows sandbox) run as a different Windows user, so $HOME and the
# user environment are not the owner's: derive the owner's paths from here.
$cockpitDir = $PSScriptRoot
$ownerHome = Split-Path (Split-Path $cockpitDir -Parent) -Parent

if (-not $env:CLAUDE_CODE_OAUTH_TOKEN) {
  $token = [Environment]::GetEnvironmentVariable('CLAUDE_CODE_OAUTH_TOKEN', 'User')
  if (-not $token) {
    # Fallback for sandboxed callers: a token file next to this script.
    $tokenFile = Join-Path $cockpitDir 'oauth-token'
    if (Test-Path -LiteralPath $tokenFile) {
      try { $token = (Get-Content -LiteralPath $tokenFile -Raw -ErrorAction Stop) -replace '\s', '' } catch { $token = $null }
    }
  }
  if ($token) { $env:CLAUDE_CODE_OAUTH_TOKEN = $token }
}
if (-not $env:CLAUDE_CODE_OAUTH_TOKEN) {
  [Console]::Error.WriteLine("claude-run: no login token found (CLAUDE_CODE_OAUTH_TOKEN is not set for this Windows user and $cockpitDir\oauth-token is missing or unreadable).")
}

function Find-Claude {
  $cmd = Get-Command claude.exe -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $cmd) { $cmd = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1 }
  if ($cmd) { return $cmd.Source }
  foreach ($homeDir in @($ownerHome, $HOME) | Select-Object -Unique) {
    $local = Join-Path $homeDir '.local\bin\claude.exe'
    if (Test-Path -LiteralPath $local) { return $local }
  }
  $roots = @()
  foreach ($packages in @((Join-Path $ownerHome 'AppData\Local\Packages'), (Join-Path $env:LOCALAPPDATA 'Packages')) | Select-Object -Unique) {
    if (Test-Path -LiteralPath $packages) {
      Get-ChildItem -LiteralPath $packages -Directory -Filter 'Claude_*' -ErrorAction SilentlyContinue | ForEach-Object {
        $roots += (Join-Path $_.FullName 'LocalCache\Roaming\Claude\claude-code')
      }
    }
  }
  $roots += (Join-Path $ownerHome 'AppData\Roaming\Claude\claude-code')
  foreach ($root in $roots) {
    if (Test-Path -LiteralPath $root) {
      $exe = Get-ChildItem -LiteralPath $root -Recurse -Filter claude.exe -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
      if ($exe) { return $exe.FullName }
    }
  }
  throw 'claude.exe was not found. Install Claude Code or the Claude desktop app.'
}

$dir = $cockpitDir
$file = Join-Path $dir 'rate-limits.json'

function Save-RateLimit($info) {
  try {
    $kind = [string]$info.rateLimitType
    $key = $null
    if ($kind -eq 'five_hour') { $key = 'fiveHour' } elseif ($kind -eq 'seven_day') { $key = 'sevenDay' } else { return }
    $used = $null
    if ($null -ne $info.utilization) {
      $u = [double]$info.utilization
      if ($u -le 1.0) { $u = $u * 100 }
      $used = $u
    } elseif ([string]$info.status -eq 'rejected') { $used = 100.0 }
    if ($null -eq $used) { return }
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $out = @{}
    if (Test-Path -LiteralPath $file) {
      try {
        $old = Get-Content -LiteralPath $file -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($old.fiveHour) { $out.fiveHour = $old.fiveHour }
        if ($old.sevenDay) { $out.sevenDay = $old.sevenDay }
      } catch { }
    }
    $out[$key] = @{ usedPercent = $used; resetsAt = $info.resetsAt; observedAt = $now }
    $out.observedAt = $now
    $out.source = 'stream'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $tmp = Join-Path $dir ('rate-limits.' + $PID + '.tmp')
    [System.IO.File]::WriteAllText($tmp, ($out | ConvertTo-Json -Compress -Depth 4), $utf8)
    Move-Item -LiteralPath $tmp -Destination $file -Force
  } catch { }
}

$exe = Find-Claude
$useStdin = $false
$passArgs = @()
foreach ($arg in $args) { if ($arg -eq '--stdin') { $useStdin = $true } else { $passArgs += $arg } }
$claudeArgs = @('-p', '--output-format', 'stream-json', '--verbose') + $passArgs
$script:resultText = $null
$script:isError = $false
$handle = {
  $line = [string]$_
  if (-not $line.StartsWith('{')) { return }
  try { $evt = $line | ConvertFrom-Json } catch { return }
  if ($evt.type -eq 'rate_limit_event' -and $evt.rate_limit_info) { Save-RateLimit $evt.rate_limit_info }
  elseif ($evt.type -eq 'result') {
    $script:resultText = $evt.result
    if ($evt.is_error) { $script:isError = $true }
    $text = [string]$evt.result
    if ($text -match 'ENOTFOUND|EAI_AGAIN|getaddrinfo|ECONNREFUSED|ETIMEDOUT|Unable to connect') {
      [Console]::Error.WriteLine('claude-run: network error (DNS or connection). If this runs in a sandbox without network access, run it with network access or outside the sandbox.')
    }
  }
}
$ErrorActionPreference = 'Continue'
if ($useStdin) {
  $stdinText = [Console]::In.ReadToEnd()
  $stdinText | & $exe @claudeArgs | ForEach-Object $handle
} else {
  # An empty pipeline gives claude a closed standard input instead of the caller's.
  @() | & $exe @claudeArgs | ForEach-Object $handle
}
$code = $LASTEXITCODE
if ($null -ne $script:resultText) { Write-Output $script:resultText }
if ($code -ne 0) { exit $code }
if ($script:isError) { exit 1 }
exit 0
