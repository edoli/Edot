# Runs in a separate, hidden Windows PowerShell process after the download.
# The application sends data through environment variables, never script interpolation.
$ErrorActionPreference = 'Stop'
$taskDirectory = [IO.Path]::GetFullPath($env:EDOT_UPDATE_DIR)
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
if (-not $taskDirectory.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
    [IO.Path]::GetFileName($taskDirectory) -notmatch '^edot-update-[0-9a-f]{32}$') {
    throw 'Invalid update directory'
}
$portable = $env:EDOT_UPDATE_SCOPE -eq 'portable'
$installer = Join-Path $taskDirectory $(if ($portable) { 'package.zip' } else { 'setup.exe' })
$executable = [IO.Path]::GetFullPath($env:EDOT_UPDATE_EXE)
$installDirectory = [IO.Path]::GetDirectoryName($executable)
$resultFile = Join-Path $env:EDOT_CONFIG_DIR 'update-result.txt'
$committed = $false
$parent = $null
$backup = $null

function Write-Result([string]$Message) {
    # Keep the result across restart, including installer failure or a canceled UAC prompt.
    [IO.File]::WriteAllText($resultFile, $Message, (New-Object Text.UTF8Encoding($false)))
}

function Verify-Installer {
    # PowerShell 7 can leave a PSModulePath that hides Windows PowerShell's Get-FileHash.
    $algorithm = [Security.Cryptography.SHA256]::Create()
    $stream = $null
    try {
        $stream = [IO.File]::OpenRead($installer)
        $hash = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '')
    } finally {
        if ($stream) { $stream.Dispose() }
        $algorithm.Dispose()
    }
    if ($hash -ne $env:EDOT_UPDATE_HASH) {
        throw '설치 파일 검증에 실패했습니다. 다시 업데이트하세요.'
    }
}

try {
    if ($env:EDOT_UPDATE_SCOPE -notin @('user', 'machine', 'portable') -or
        $env:EDOT_UPDATE_VERSION -notmatch '^\d+\.\d+\.\d+$' -or
        $env:EDOT_UPDATE_HASH -notmatch '^[0-9a-fA-F]{64}$') {
        throw '잘못된 업데이트 정보입니다.'
    }
    $parent = [Diagnostics.Process]::GetProcessById([int]$env:EDOT_UPDATE_PARENT)
    # Acquire a real process handle before announcing readiness, avoiding PID reuse races.
    $null = $parent.Handle
    if ($parent.MainModule.FileName -ne $executable) {
        throw '업데이트할 프로그램 경로가 일치하지 않습니다.'
    }
    Verify-Installer
    [IO.File]::WriteAllText((Join-Path $taskDirectory 'ready'), 'ready')
    $deadline = [DateTime]::UtcNow.AddMinutes(5)
    while (-not [IO.File]::Exists((Join-Path $taskDirectory 'go'))) {
        if ($parent.HasExited) {
            # The app creates go before exiting; recheck after observing exit.
            if ([IO.File]::Exists((Join-Path $taskDirectory 'go'))) { break }
            return
        }
        if ([DateTime]::UtcNow -gt $deadline) { return }
        Start-Sleep -Milliseconds 50
    }
    $committed = $true
    if (-not $parent.WaitForExit(30000)) {
        # Do not terminate an editor whose data may not have been saved.
        $committed = $false
        throw '앱이 종료되지 않았습니다. 메모 저장 상태를 확인한 뒤 다시 시도하세요.'
    }
    # Fail before running Setup if another Space/process still has this EXE loaded.
    if ($portable -or $env:EDOT_UPDATE_SCOPE -eq 'user') {
    try {
        $probe = [IO.File]::Open($executable, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $probe.Dispose()
    } catch {
        throw '다른 Edot 창이 열려 있습니다. 모든 창을 닫고 다시 업데이트하세요.'
    }
    }
    Verify-Installer
    if ($portable) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [IO.Compression.ZipFile]::OpenRead($installer)
        try {
            $entries = @($archive.Entries | Where-Object { $_.FullName -ceq 'edot.exe' })
            if ($entries.Count -ne 1 -or ($entries[0].ExternalAttributes -shr 16 -band 0xF000) -eq 0xA000) { throw 'Invalid portable executable' }
            $newExecutable = Join-Path $taskDirectory 'edot.exe'
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $newExecutable)
        } finally { $archive.Dispose() }
        if ((Get-Item -LiteralPath $newExecutable).VersionInfo.ProductVersion -ne $env:EDOT_UPDATE_VERSION) { throw 'Downloaded executable version does not match' }
        $backup = Join-Path $taskDirectory 'previous.exe'
        [IO.File]::Copy($executable, $backup)
        try { [IO.File]::Copy($newExecutable, $executable, $true) }
        catch { [IO.File]::Copy($backup, $executable, $true); throw }
    } else {
    $scopeArgument = if ($env:EDOT_UPDATE_SCOPE -eq 'machine') { '/ALLUSERS' } else { '/CURRENTUSER' }
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/SP-', '/NORESTART',
        '/RESTARTEXITCODE=3010', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS',
        $scopeArgument, ('/DIR="' + $installDirectory + '"'),
        ('/LOG="' + (Join-Path $taskDirectory 'install.log') + '"'))
    $options = @{ FilePath = $installer; ArgumentList = $arguments; Wait = $true; PassThru = $true }
    if ($env:EDOT_UPDATE_SCOPE -eq 'machine') {
        # Keep the helper itself unelevated, so restart always uses the original user.
        $options.Verb = 'RunAs'
    } else {
        $options.WindowStyle = 'Hidden'
    }
    $setup = Start-Process @options
    if ($setup.ExitCode -ne 0) { throw "설치가 완료되지 않았습니다 (코드 $($setup.ExitCode))." }
    }
    if ((Get-Item -LiteralPath $executable).VersionInfo.ProductVersion -ne $env:EDOT_UPDATE_VERSION) {
        throw '설치한 버전이 일치하지 않습니다. Release 페이지에서 다시 설치하세요.'
    }
    # A successful update clears older errors; the version row already shows the installed version.
    Write-Result ''
} catch {
    if ($backup -and [IO.File]::Exists($backup)) { try { [IO.File]::Copy($backup, $executable, $true) } catch { } }
    try { Write-Result ('업데이트 실패: ' + $_.Exception.Message) } catch { }
} finally {
    if ($parent) { $parent.Dispose() }
    if ($committed) {
        # EDOT_SPACE_DIR and EDOT_CONFIG_DIR remain inherited, restoring the active Space.
        try {
            Start-Process -FilePath $executable -WorkingDirectory $installDirectory -WindowStyle Normal
        } catch {
            try { Write-Result ('앱을 다시 실행하지 못했습니다: ' + $_.Exception.Message) } catch { }
        }
    }
    # Delete only files owned by this operation; never recursively delete an install/data folder.
    foreach ($name in @('setup.exe', 'package.zip', 'edot.exe', 'previous.exe', 'update.ps1', 'ready', 'go.pending', 'go', 'install.log')) {
        Remove-Item -LiteralPath (Join-Path $taskDirectory $name) -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $taskDirectory -ErrorAction SilentlyContinue
}
