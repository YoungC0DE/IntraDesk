#Requires -Version 5.1
<#
.SYNOPSIS
    Prepara uma máquina Windows 10/11 x64 para compilar o IntraDesk.

.DESCRIPTION
    Instala (ou reaproveita, se já estiverem na versão certa):
      - Git, Python 3.12, Visual Studio 2022 Build Tools (C++)
      - LLVM 15.0.6 (libclang para o bindgen)
      - Rust 1.75 (MSVC), fixado neste repositório com `rustup override`
      - Flutter 3.24.5 com o engine customizado do RustDesk e o patch do dropdown
      - vcpkg no commit do CI e as bibliotecas C/C++ (x64-windows-static)
      - cargo-expand e flutter_rust_bridge_codegen (para gerar o bridge)

    As versões precisam bater com .github/workflows/build-windows.yml.
    Pode ser executado de novo com segurança: o que já está instalado é pulado.

.PARAMETER DevRoot
    Pasta onde ficam Flutter, vcpkg e LLVM. Padrão: C:\dev

.PARAMETER SkipVcpkgDeps
    Não compila as bibliotecas do vcpkg (a primeira vez leva mais de 1 hora por causa do ffmpeg).

.PARAMETER SkipBridgeTools
    Não instala cargo-expand e flutter_rust_bridge_codegen.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\setup-dev.ps1
#>
param(
    [string]$DevRoot = 'C:\dev',
    [switch]$SkipVcpkgDeps,
    [switch]$SkipBridgeTools
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Keep in sync with .github/workflows/build-windows.yml and bridge.yml.
$RustVersion = '1.75'
$LlvmVersion = '15.0.6'
$FlutterVersion = '3.24.5'
$VcpkgCommit = '9e593bb18ea69cc5095e012465dcd675a822ed0d'
$VcpkgTriplet = 'x64-windows-static'
$CargoExpandVersion = '1.0.95'
$FrbVersion = '1.80.1'

$RepoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor DarkYellow
}

function Write-Ok([string]$Message) {
    Write-Host "    $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host "    AVISO: $Message" -ForegroundColor Yellow
}

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Falhou ($LASTEXITCODE): $Exe $($Arguments -join ' ')"
    }
}

function Update-SessionPath {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:Path = "$machine;$user"
}

function Set-UserEnv([string]$Name, [string]$Value) {
    [Environment]::SetEnvironmentVariable($Name, $Value, 'User')
    Set-Item -Path "Env:$Name" -Value $Value
    Write-Ok "$Name = $Value"
}

function Add-UserPath([string]$Dir) {
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($user) { $parts = $user.Split(';') | Where-Object { $_ } }
    if ($parts -notcontains $Dir) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $Dir) -join ';'), 'User')
        Write-Ok "Adicionado ao PATH do usuário: $Dir"
    }
    Update-SessionPath
}

function Test-Command([string]$Name) {
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    # The WindowsApps python stubs only open the Microsoft Store.
    return [bool]($cmd -and $cmd.Source -notlike '*\WindowsApps\*')
}

function Install-WingetPackage([string]$Id, [string[]]$Extra = @()) {
    & winget list --id $Id -e --accept-source-agreements *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Ok "$Id já instalado"
        return
    }
    Write-Host "    Instalando $Id via winget..."
    $wingetArgs = @('install', '--id', $Id, '-e', '--silent',
        '--accept-package-agreements', '--accept-source-agreements') + $Extra
    Invoke-Native 'winget' $wingetArgs
    Update-SessionPath
}

function Save-Download([string]$Url, [string]$OutFile) {
    Write-Host "    Baixando $Url"
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
}

# --- Pré-checagens -----------------------------------------------------------

Write-Step 'Verificando o sistema'
if ([Environment]::OSVersion.Version.Major -lt 10 -or -not [Environment]::Is64BitOperatingSystem) {
    throw 'É necessário Windows 10 ou 11 x64.'
}
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw 'winget não encontrado. Instale o "App Installer" pela Microsoft Store e rode de novo.'
}
# Tools installed by an earlier run are on the user PATH but not in this terminal's PATH yet.
Update-SessionPath
New-Item -ItemType Directory -Force -Path $DevRoot | Out-Null
Write-Ok "Repositório: $RepoRoot"
Write-Ok "Ferramentas em: $DevRoot"

# --- Git ---------------------------------------------------------------------

Write-Step 'Git'
if (Test-Command 'git') { Write-Ok 'git encontrado' } else { Install-WingetPackage 'Git.Git' }
Invoke-Native 'git' @('-C', $RepoRoot, 'submodule', 'update', '--init', '--recursive')
Write-Ok 'Submódulos atualizados (libs/hbb_common)'

# --- Visual Studio Build Tools ----------------------------------------------

Write-Step 'Visual Studio 2022 Build Tools (C++)'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = $null
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if ($vsPath) {
    Write-Ok "Ferramentas C++ encontradas em $vsPath"
} else {
    # In passive mode the VS installer cancels itself when any Visual Studio is open.
    if (Get-Process -Name devenv -ErrorAction SilentlyContinue) {
        throw ('O Visual Studio está aberto e bloqueia a instalação do Build Tools. ' +
            'Salve seu trabalho, feche o Visual Studio e rode o script de novo.')
    }
    $override = '--wait --passive --add Microsoft.VisualStudio.Workload.VCTools ' +
        '--add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 ' +
        '--add Microsoft.VisualStudio.Component.VC.CMake.Project ' +
        '--add Microsoft.VisualStudio.Component.Windows11SDK.22621 --includeRecommended'
    try {
        Install-WingetPackage 'Microsoft.VisualStudio.2022.BuildTools' @('--override', $override)
    } catch {
        # The VS installer reports "reboot required" and similar as failures; vswhere below is the real check.
        Write-Warn $_.Exception.Message
    }
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vsPath) {
        throw ('Ferramentas C++ do Build Tools 2022 não instaladas. Veja os logs dd_*.log em %TEMP%. ' +
            'Se o Build Tools 2022 já existia, abra o "Visual Studio Installer", clique em Modificar ' +
            'e marque "Desenvolvimento para desktop com C++".')
    }
    Write-Ok "Ferramentas C++ instaladas em $vsPath"
}

# --- Python ------------------------------------------------------------------

Write-Step 'Python 3'
if (-not (Test-Command 'python')) {
    Install-WingetPackage 'Python.Python.3.12'
}
$python = (Get-Command python -All | Where-Object { $_.Source -notlike '*\WindowsApps\*' } |
    Select-Object -First 1).Source
if (-not $python) {
    throw 'Python não encontrado no PATH. Abra um novo terminal e rode o script de novo.'
}
Write-Ok "python: $python"
# build.py calls `python3` and `pip3`, which the Windows installer does not create.
$python3 = Join-Path (Split-Path $python) 'python3.exe'
if (-not (Test-Path $python3)) {
    try {
        Copy-Item $python $python3
        Write-Ok "Criado $python3"
    } catch {
        Write-Warn "Não foi possível criar $python3 (rode como administrador ou crie a cópia manualmente)."
    }
}
if ((Get-Command python3 -ErrorAction SilentlyContinue).Source -like '*\WindowsApps\*') {
    Write-Warn ('"python3" está abrindo o atalho da Microsoft Store. Desative-o em Configurações > ' +
        'Aplicativos > Configurações avançadas > Aliases de execução do aplicativo.')
}

# --- LLVM --------------------------------------------------------------------

Write-Step "LLVM $LlvmVersion"
$llvmDir = Join-Path $DevRoot "llvm-$LlvmVersion"
if (Test-Path (Join-Path $llvmDir 'bin\libclang.dll')) {
    Write-Ok "Já instalado em $llvmDir"
} else {
    $installer = Join-Path $env:TEMP "LLVM-$LlvmVersion-win64.exe"
    Save-Download "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LlvmVersion/LLVM-$LlvmVersion-win64.exe" $installer
    # NSIS requires /D to be last and unquoted.
    $p = Start-Process -FilePath $installer -ArgumentList '/S', "/D=$llvmDir" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "Instalador do LLVM retornou $($p.ExitCode)" }
    Remove-Item $installer
}
Set-UserEnv 'LIBCLANG_PATH' (Join-Path $llvmDir 'bin')

# --- Rust --------------------------------------------------------------------

Write-Step "Rust $RustVersion (MSVC)"
if (-not (Test-Command 'rustup')) {
    Install-WingetPackage 'Rustlang.Rustup'
    Add-UserPath (Join-Path $env:USERPROFILE '.cargo\bin')
}
# The self-update replaces rustup.exe in the background, so the next rustup call fails with "file in use".
Invoke-Native 'rustup' @('toolchain', 'install', $RustVersion, '--no-self-update', '--profile', 'minimal',
    '--component', 'rustfmt', '--target', 'x86_64-pc-windows-msvc')
Push-Location $RepoRoot
try {
    Invoke-Native 'rustup' @('override', 'set', $RustVersion)
} finally {
    Pop-Location
}
Write-Ok "Repositório fixado em Rust $RustVersion"

# --- Flutter -----------------------------------------------------------------

Write-Step "Flutter $FlutterVersion"
$flutterDir = Join-Path $DevRoot 'flutter'
$flutterBin = Join-Path $flutterDir 'bin'
$flutterOk = $false
if (Test-Path (Join-Path $flutterBin 'flutter.bat')) {
    $ver = (& (Join-Path $flutterBin 'flutter.bat') --version --suppress-analytics 2>$null | Out-String)
    $flutterOk = $ver -match "Flutter $([regex]::Escape($FlutterVersion))\b"
    if (-not $flutterOk) {
        throw "Existe outro Flutter em $flutterDir. Remova a pasta ou use -DevRoot com outra pasta."
    }
}
if ($flutterOk) {
    Write-Ok "Já instalado em $flutterDir"
} else {
    $zip = Join-Path $env:TEMP "flutter_windows_$FlutterVersion-stable.zip"
    Save-Download "https://storage.googleapis.com/flutter_infra_release/releases/stable/windows/flutter_windows_$FlutterVersion-stable.zip" $zip
    Write-Host '    Extraindo (alguns minutos)...'
    Invoke-Native 'tar' @('-xf', $zip, '-C', $DevRoot)
    Remove-Item $zip
}
Add-UserPath $flutterBin
Invoke-Native 'flutter' @('config', '--no-analytics', '--enable-windows-desktop')
Invoke-Native 'flutter' @('precache', '--windows')

# Same engine swap and patch as the CI build, see .github/workflows/build-windows.yml.
$engineMarker = Join-Path $flutterDir '.intradesk-engine'
if (Test-Path $engineMarker) {
    Write-Ok 'Engine customizado e patch já aplicados'
} else {
    $engineZip = Join-Path $env:TEMP 'windows-x64-release.zip'
    $engineTmp = Join-Path $env:TEMP 'windows-x64-release'
    Save-Download 'https://github.com/rustdesk/engine/releases/download/main/windows-x64-release.zip' $engineZip
    if (Test-Path $engineTmp) { Remove-Item -Recurse -Force $engineTmp }
    Expand-Archive -Path $engineZip -DestinationPath $engineTmp
    $engineDest = Join-Path $flutterDir 'bin\cache\artifacts\engine\windows-x64-release'
    Copy-Item -Path (Join-Path $engineTmp '*') -Destination $engineDest -Recurse -Force
    Remove-Item -Recurse -Force $engineTmp, $engineZip
    Write-Ok 'Engine customizado do RustDesk instalado'

    $patch = Join-Path $RepoRoot '.github\patches\flutter_3.24.4_dropdown_menu_enableFilter.diff'
    Invoke-Native 'git' @('-C', $flutterDir, 'apply', $patch)
    Write-Ok 'Patch do dropdown aplicado'
    Set-Content -Path $engineMarker -Value $FlutterVersion
}

# --- Modo de desenvolvedor ---------------------------------------------------

Write-Step 'Modo de desenvolvedor do Windows (symlinks dos plugins Flutter)'
$devMode = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' `
    -Name AllowDevelopmentWithoutDevLicense -ErrorAction SilentlyContinue
if ($devMode -and $devMode.AllowDevelopmentWithoutDevLicense -eq 1) {
    Write-Ok 'Ativado'
} else {
    Write-Warn 'Desativado. Ative em Configurações > Para desenvolvedores (abrindo agora).'
    Start-Process 'ms-settings:developers'
}

# --- vcpkg -------------------------------------------------------------------

Write-Step 'vcpkg'
$vcpkgDir = Join-Path $DevRoot 'vcpkg'
if (-not (Test-Path (Join-Path $vcpkgDir '.git'))) {
    Invoke-Native 'git' @('clone', 'https://github.com/microsoft/vcpkg', $vcpkgDir)
}
$current = (& git -C $vcpkgDir rev-parse HEAD).Trim()
if ($current -ne $VcpkgCommit) {
    Invoke-Native 'git' @('-C', $vcpkgDir, 'fetch', 'origin')
    Invoke-Native 'git' @('-C', $vcpkgDir, 'checkout', $VcpkgCommit)
}
$vcpkgExe = Join-Path $vcpkgDir 'vcpkg.exe'
if (-not (Test-Path $vcpkgExe) -or $current -ne $VcpkgCommit) {
    Invoke-Native (Join-Path $vcpkgDir 'bootstrap-vcpkg.bat') @('-disableMetrics')
}
Set-UserEnv 'VCPKG_ROOT' $vcpkgDir

if ($SkipVcpkgDeps) {
    Write-Warn 'Bibliotecas do vcpkg puladas (-SkipVcpkgDeps).'
} else {
    Write-Host '    Compilando bibliotecas (a primeira vez leva mais de 1 hora)...'
    $env:VCPKG_DEFAULT_HOST_TRIPLET = $VcpkgTriplet
    Push-Location $RepoRoot
    try {
        Invoke-Native $vcpkgExe @('install', '--triplet', $VcpkgTriplet,
            "--x-install-root=$(Join-Path $vcpkgDir 'installed')")
    } finally {
        Pop-Location
    }
    Write-Ok 'Bibliotecas instaladas'
}

# --- Ferramentas do bridge ---------------------------------------------------

Write-Step 'Ferramentas do flutter_rust_bridge'
if ($SkipBridgeTools) {
    Write-Warn 'Puladas (-SkipBridgeTools).'
} else {
    $frbVer = ''
    if (Test-Command 'flutter_rust_bridge_codegen') {
        $frbVer = (& flutter_rust_bridge_codegen --version | Out-String)
    }
    if ($frbVer -match [regex]::Escape($FrbVersion)) {
        Write-Ok "flutter_rust_bridge_codegen $FrbVersion já instalado"
    } else {
        Invoke-Native 'cargo' @("+$RustVersion", 'install', 'cargo-expand',
            '--version', $CargoExpandVersion, '--locked')
        Invoke-Native 'cargo' @("+$RustVersion", 'install', 'flutter_rust_bridge_codegen',
            '--version', $FrbVersion, '--features', 'uuid', '--locked')
    }
}

# --- Resumo ------------------------------------------------------------------

Write-Step 'flutter doctor'
& flutter doctor

Write-Host ''
Write-Host 'Ambiente pronto.' -ForegroundColor Green
Write-Host 'Abra um NOVO terminal (para carregar PATH e variáveis) e rode:'
Write-Host '    .\scripts\build.ps1'
