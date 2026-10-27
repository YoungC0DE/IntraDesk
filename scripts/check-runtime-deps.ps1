#Requires -Version 5.1
<#
.SYNOPSIS
    Falha se algum .exe ou .dll da pasta depende do Visual C++ Redistributable.

.DESCRIPTION
    O IntraDesk precisa abrir num Windows 10 limpo, sem o VC++ Redistributable (T6.4).
    Hoje todos os binários linkam o CRT de forma estática (Rust com +crt-static em
    .cargo\config.toml; runner e plugins Flutter com /MT). Este script confere as
    importações com dumpbin e acusa vcruntime*, msvcp*, vcomp* e concrt*. O UCRT
    (api-ms-win-crt-*, ucrtbase) faz parte do Windows 10 e não conta. Roda no
    scripts\build.ps1 e no CI.

.PARAMETER Path
    Pasta do build (ex.: flutter\build\windows\x64\runner\Release).

.EXAMPLE
    .\scripts\check-runtime-deps.ps1 flutter\build\windows\x64\runner\Release
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Path
)

$ErrorActionPreference = 'Stop'

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = & $vswhere -latest -products * -version '[17.0,18.0)' `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) {
    throw 'Visual Studio 2022 com ferramentas C++ não encontrado (precisa do dumpbin).'
}
$dumpbin = Get-ChildItem (Join-Path $vsPath 'VC\Tools\MSVC') -Recurse -Filter 'dumpbin.exe' |
    Where-Object { $_.FullName -match '\\Hostx64\\x64\\' } |
    Select-Object -First 1
if (-not $dumpbin) {
    throw "dumpbin.exe não encontrado em $vsPath"
}

$pattern = '^\s+(vcruntime|msvcp|vcomp|concrt)\S*\.dll\s*$'
$binaries = Get-ChildItem -Path $Path -Recurse -File -Include '*.exe', '*.dll'
if (-not $binaries) {
    throw "Nenhum .exe/.dll em $Path"
}
$found = @()
foreach ($file in $binaries) {
    $deps = & $dumpbin.FullName /nologo /dependents $file.FullName
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin falhou em $($file.FullName)"
    }
    $hits = @($deps | Where-Object { $_ -match $pattern } | ForEach-Object { $_.Trim() })
    if ($hits.Count -gt 0) {
        $found += "    $($file.Name): $($hits -join ', ')"
    }
}
if ($found.Count -gt 0) {
    $found | ForEach-Object { Write-Host $_ }
    throw 'Binários dependem do VC++ Redistributable (veja acima). Linke o CRT estático.'
}
Write-Host "OK: $($binaries.Count) binários sem dependência do VC++ Redistributable."
