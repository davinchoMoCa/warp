#!/usr/bin/env pwsh
#
# Instala o actualiza Moca Warp en Windows.
#
# Uso:
#   .\script\moca\instalar.ps1              # baja el instalador del último GitHub Release
#   .\script\moca\instalar.ps1 -Version 0.2.0
#   .\script\moca\instalar.ps1 -Compilar    # compila local lo que haya en el checkout
#   .\script\moca\instalar.ps1 -Compilar -Pull -Publicar   # robcod14: compila la última
#                                                         # versión y la sube al Release
#
# Las versiones se sacan desde la Mac con script/moca/release. El instalador de Windows se
# compila en robcod14 con -Compilar -Pull -Publicar y las demás máquinas (Surface) lo bajan
# del Release con el uso sin parámetros. El workflow moca-release.yml queda de respaldo
# (se corre a mano desde GitHub Actions).
#
# -Compilar requiere Visual Studio Build Tools (C++), rustup, protoc, Inno Setup 6 y cargo-about.
# -Publicar requiere además gh con sesión iniciada (gh auth login).

Param(
    # Versión a instalar (X.Y.Z). Por defecto, la del último Release.
    [String]$Version = '',
    # Reinstala aunque esa versión ya esté instalada.
    [Switch]$Forzar,
    # Compila local con script/windows/bundle.ps1 en vez de descargar.
    [Switch]$Compilar,
    # Con -Compilar: hace git pull de la rama moca antes de compilar.
    [Switch]$Pull,
    # Con -Compilar: sube el instalador al GitHub Release vX.Y.Z (lo crea si no existe).
    [Switch]$Publicar
)

$ErrorActionPreference = 'Stop'

$Repo = 'davinchoMoCa/warp'
$RepoRoot = (Get-Item "$PSScriptRoot\..\..").FullName
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\warp-terminal-oss_is1'

function Install-MocaWarp([String]$Installer) {
    Write-Output '==> Cerrando Moca Warp si está abierta'
    Get-Process warp-oss -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -notlike "$RepoRoot\*" } |
        Stop-Process -Force
    Start-Sleep -Seconds 2

    Write-Output '==> Instalando'
    # WaitForExit en vez de Start-Process -Wait: -Wait también espera a los hijos, y el
    # instalador a veces deja abierta la app (entrada postinstall de windows-installer.iss).
    $Proc = Start-Process $Installer -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', '/MERGETASKS=!desktopicon' -PassThru
    $Proc.WaitForExit()
    if ($Proc.ExitCode -ne 0) { throw "El instalador terminó con código $($Proc.ExitCode)." }

    $Installed = Get-ItemProperty $UninstallKey -ErrorAction SilentlyContinue
    Write-Output "Listo: Moca Warp $($Installed.DisplayVersion) instalada en $($Installed.InstallLocation)"

    $Exe = Join-Path $Installed.InstallLocation 'warp-oss.exe'
    Start-Sleep -Seconds 5
    if (-not (Get-Process warp-oss -ErrorAction SilentlyContinue | Where-Object Path -eq $Exe)) {
        Start-Process $Exe
    }
}

$Current = (Get-ItemProperty $UninstallKey -ErrorAction SilentlyContinue).DisplayVersion

if (-not $Compilar) {
    $Api = "https://api.github.com/repos/$Repo/releases/" + $(if ($Version) { "tags/v$Version" } else { 'latest' })
    try {
        $Release = Invoke-RestMethod $Api -Headers @{ 'User-Agent' = 'moca-warp-instalar' }
    } catch {
        throw "No encontré el Release ($Api). ¿Ya se publicó desde la Mac?"
    }
    $Asset = $Release.assets | Where-Object name -like '*-Windows-x64-Setup.exe' | Select-Object -First 1
    if (-not $Asset) {
        throw "El Release $($Release.tag_name) todavía no tiene instalador de Windows. Revisa https://github.com/$Repo/actions/workflows/moca-release.yml"
    }
    if ($Current -eq $Release.tag_name -and -not $Forzar) {
        Write-Output "Moca Warp $Current ya está instalada (usa -Forzar para reinstalar)."
        exit 0
    }

    Write-Output "==> Descargando $($Asset.name) ($Current -> $($Release.tag_name))"
    $Installer = Join-Path $env:TEMP $Asset.name
    Invoke-WebRequest $Asset.browser_download_url -OutFile $Installer
    Install-MocaWarp $Installer
    Remove-Item $Installer -Force
    exit 0
}

Set-Location $RepoRoot

$env:Path = @(
    [Environment]::GetEnvironmentVariable('Path', 'Machine'),
    [Environment]::GetEnvironmentVariable('Path', 'User'),
    "$env:LOCALAPPDATA\Programs\Inno Setup 6",
    "${env:ProgramFiles(x86)}\Inno Setup 6"
) -join ';'

$Tools = @('cargo', 'protoc', 'ISCC', 'cargo-about') + $(if ($Publicar) { @('gh') } else { @() })
foreach ($tool in $Tools) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Falta $tool. Revisa los requisitos al inicio de este script."
    }
}

if ($Pull) {
    if ((git branch --show-current) -ne 'moca') { throw '-Pull solo funciona en la rama moca.' }
    Write-Output '==> git pull'
    git pull --ff-only origin moca
    if (-not $?) { throw 'git pull falló.' }
}

$LocalVersion = (Select-String -Path 'app\Cargo.toml' -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$env:GIT_RELEASE_TAG = "v$LocalVersion"
Write-Output "==> Compilando Moca Warp v$LocalVersion ($(git rev-parse --short HEAD)) en release (la primera vez tarda bastante)"

& "$RepoRoot\script\windows\bundle.ps1" -CHANNEL oss
if (-not $?) { throw 'La compilación falló.' }

$Built = "$RepoRoot\script\windows\Output\MocaWarpSetup.exe"

if ($Publicar) {
    $Tag = "v$LocalVersion"
    $Asset = Join-Path $env:TEMP "MocaWarp-$LocalVersion-Windows-x64-Setup.exe"
    Copy-Item $Built $Asset -Force
    Write-Output "==> Subiendo $(Split-Path $Asset -Leaf) al Release $Tag"
    gh release view $Tag --repo $Repo *> $null
    if (-not $?) {
        gh release create $Tag --repo $Repo --title "Moca Warp $Tag" --notes "Moca Warp $Tag"
        if (-not $?) { throw "No pude crear el Release $Tag." }
    }
    gh release upload $Tag $Asset --repo $Repo --clobber
    if (-not $?) { throw "No pude subir el instalador al Release $Tag." }
    Remove-Item $Asset -Force
}

Install-MocaWarp $Built
