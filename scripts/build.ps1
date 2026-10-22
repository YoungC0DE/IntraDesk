#Requires -Version 5.1
<#
.SYNOPSIS
    Compila o IntraDesk localmente e gera o executável portátil em dist\.

.DESCRIPTION
    Mesmos passos do CI (.github/workflows/build-windows.yml):
      1. gera o bridge Flutter <-> Rust (se src\flutter_ffi.rs mudou ou ainda não existe);
      2. compila Rust + Flutter com build.py;
      3. empacota tudo em um único .exe com libs\portable.
    Requer o ambiente do scripts\setup-dev.ps1.

.PARAMETER ForceBridge
    Regenera o bridge mesmo sem mudanças em src\flutter_ffi.rs.

.PARAMETER NoPack
    Para depois do build; a pasta fica em flutter\build\windows\x64\runner\Release.

.EXAMPLE
    .\scripts\build.ps1
#>
param(
    [switch]$ForceBridge,
    [switch]$NoPack
)

$ErrorActionPreference = 'Stop'

# Keep in sync with BUILD_ARGS in .github/workflows/build-windows.yml.
$BuildArgs = @('--portable', '--flutter', '--skip-portable-pack', '--hwcodec', '--vram')

$RepoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor DarkYellow
}

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Falhou ($LASTEXITCODE): $Exe $($Arguments -join ' ')"
    }
}

foreach ($var in 'VCPKG_ROOT', 'LIBCLANG_PATH') {
    if (-not (Test-Path "Env:$var")) {
        throw "$var não definido. Rode scripts\setup-dev.ps1 e abra um novo terminal."
    }
}

Push-Location $RepoRoot
try {
    Write-Step 'Bridge Flutter <-> Rust'
    $ffi = Get-Item 'src\flutter_ffi.rs'
    $dartBridge = 'flutter\lib\generated_bridge.dart'
    $stale = $ForceBridge -or -not (Test-Path $dartBridge) -or
        -not (Test-Path 'src\bridge_generated.rs') -or
        ((Get-Item $dartBridge).LastWriteTime -lt $ffi.LastWriteTime)
    if ($stale) {
        Push-Location flutter
        try { Invoke-Native 'flutter' @('pub', 'get') } finally { Pop-Location }
        New-Item -ItemType Directory -Force -Path 'build\bridge' | Out-Null
        Invoke-Native 'flutter_rust_bridge_codegen' @(
            '--rust-input', '.\src\flutter_ffi.rs',
            '--dart-output', ".\$dartBridge",
            '--c-output', '.\build\bridge\bridge_generated.h',
            '--llvm-path', (Split-Path $env:LIBCLANG_PATH))
    } else {
        Write-Host '    Atualizado, pulando (use -ForceBridge para regenerar).'
    }

    Write-Step 'Build (Rust + Flutter)'
    Invoke-Native 'python' (@('build.py') + $BuildArgs)
    $releaseDir = 'flutter\build\windows\x64\runner\Release'
    if ($NoPack) {
        Write-Host "Pronto: $RepoRoot\$releaseDir\rustdesk.exe" -ForegroundColor Green
        return
    }

    Write-Step 'Executável portátil'
    $runnerRes = Get-ChildItem -Path 'flutter\build' -Filter 'Runner.res' -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($runnerRes) { Copy-Item $runnerRes.FullName 'libs\portable\Runner.res' -Force }

    # The packer embeds res\manifest.xml; drop dpiAware like the CI does, then restore it.
    $manifest = 'res\manifest.xml'
    $original = [IO.File]::ReadAllText((Resolve-Path $manifest))
    try {
        $patched = ($original -split "`r?`n" | Where-Object { $_ -notmatch 'dpiAware' }) -join "`n"
        [IO.File]::WriteAllText((Resolve-Path $manifest), $patched)
        Push-Location 'libs\portable'
        try {
            Invoke-Native 'python' @('-m', 'pip', 'install', '-q', '-r', 'requirements.txt')
            Invoke-Native 'python' @('.\generate.py', '-f', "..\..\$releaseDir\", '-o', '.',
                '-e', "..\..\$releaseDir\rustdesk.exe")
        } finally {
            Pop-Location
        }
    } finally {
        [IO.File]::WriteAllText((Resolve-Path $manifest), $original)
    }

    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    $out = 'dist\IntraDesk-dev.exe'
    Move-Item -Force 'target\release\rustdesk-portable-packer.exe' $out
    Write-Host ''
    Write-Host "Pronto: $RepoRoot\$out" -ForegroundColor Green
} finally {
    Pop-Location
}
