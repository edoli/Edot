param(
    [string]$Version,
    [string]$Repository = 'edoli/Edot',
    [string]$BuildLabel = '',
    [string]$Compiler = $env:INNO_SETUP_COMPILER,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\target\dist')
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$cargoVersion = (& python -c "import sys, tomllib; print(tomllib.load(open(sys.argv[1], 'rb'))['package']['version'])" (Join-Path $projectRoot 'Cargo.toml'))
if ($LASTEXITCODE -ne 0) { throw 'Cannot read Cargo.toml version' }
if ($PSBoundParameters.ContainsKey('Version') -and $Version -cne $cargoVersion) {
    throw "Requested version '$Version' does not match Cargo.toml version '$cargoVersion'"
}
$Version = $cargoVersion
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or
    @($Version.Split('.') | Where-Object { [long]$_ -gt 65535 }).Count -ne 0) {
    throw 'Version must be major.minor.patch, with each component <= 65535'
}
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Invalid GitHub repository' }
if (-not $Compiler) {
    $candidates = @((Join-Path $projectRoot 'target\tools\InnoSetup\ISCC.exe'),
        'C:\Program Files (x86)\Inno Setup 6\ISCC.exe', 'C:\Program Files\Inno Setup 7\ISCC.exe')
    $Compiler = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $Compiler -or -not (Test-Path -LiteralPath $Compiler)) {
    throw 'Install Inno Setup first: powershell -File tools/install_inno.ps1'
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$previous = @{}
foreach ($name in @('EDOT_REPOSITORY', 'EDOT_BUILD_LABEL')) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
Push-Location $projectRoot
try {
    $env:EDOT_REPOSITORY = $Repository
    $env:EDOT_BUILD_LABEL = $BuildLabel
    & cargo build --release --target-dir target
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $executable = Join-Path $projectRoot 'target\release\edot.exe'
    if ((Get-Item -LiteralPath $executable).VersionInfo.ProductVersion -ne $Version) {
        throw 'EXE version does not match the package version'
    }
    & $Compiler "/DAppVersion=$Version" "/DSourceExe=$executable" "/DOutputPath=$OutputDirectory" "/DReleaseRepository=$Repository" (Join-Path $projectRoot 'installer\edot.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }
    $installer = Join-Path $OutputDirectory "Edot-Setup-$Version-windows-x64.exe"
    $portable = Join-Path $OutputDirectory "Edot-$Version-windows-x64.zip"
    # ZIP contains the EXE only; installation registration and updater helpers are not bundled.
    Compress-Archive -LiteralPath $executable -DestinationPath $portable -CompressionLevel Optimal -Force
    $size = (Get-Item -LiteralPath $installer).Length
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifest = "EDOTUPDATE1`nversion=$Version`nsize=$size`nsha256=$hash`n"
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'edot-update.txt'), $manifest, (New-Object Text.UTF8Encoding($false)))
    $checksums = foreach ($path in @($installer, $portable, (Join-Path $OutputDirectory 'edot-update.txt'))) {
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + [IO.Path]::GetFileName($path)
    }
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'SHA256SUMS.txt'), [string[]]$checksums, (New-Object Text.UTF8Encoding($false)))
    Get-Item -LiteralPath $installer, $portable | Select-Object Name, Length, @{ Name = 'MiB'; Expression = { [Math]::Round($_.Length / 1MB, 2) } }
} finally {
    Pop-Location
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
