[CmdletBinding()]
param(
    [switch]$Uninstall,
    [switch]$Purge,
    [string]$Name = '',
    [string]$Zone = 'wirehaus.dev',
    [string]$ViewerOrigin = 'https://app.wirehaus.dev',
    [string]$PublicHost = '',
    [string]$TunnelTokenFile = '',
    [string]$ConnectorFile = '',
    [string]$FetchConnector = '',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Wirehaus')
)

$ErrorActionPreference = 'Stop'
try {
    [Console]::OutputEncoding = [Text.Encoding]::UTF8
    $OutputEncoding = [Text.Encoding]::UTF8
} catch { }
$spanish = (Get-UICulture).TwoLetterISOLanguageName -eq 'es'
function Say([string]$es, [string]$en) {
    if ($spanish) { Write-Host $es } else { Write-Host $en }
}
$source = Split-Path -Parent $MyInvocation.MyCommand.Path
$agent = Join-Path $InstallDir 'wirehaus-agent.exe'
$dataDir = Join-Path $env:LOCALAPPDATA 'Wirehaus'
$stateFile = Join-Path $dataDir 'state\state'
$files = @('wirehaus-agent.exe', 'wirehaus-agentd.exe')
$cloudflaredVersion = '2026.10.4'
$cloudflaredSha256 = '411b16d40c8d872243efeb52c550053d00af0a8a97fbb32e730fb8aba429eaa8'
$cloudflaredGzSha256 = 'b924c817cc2ef5e679c0b20a95cb87291634f1f65ebbf1f64a9ddd5a74ee3db3'
$siteOrigin = 'https://wirehaus.dev'
$cloudflared = Join-Path $InstallDir 'cloudflared.exe'
$tunnelToken = Join-Path $dataDir 'tunnel-token'
$tunnelTask = 'WirehausTunnel'

function Remove-Tunnel {
    if (Get-ScheduledTask -TaskName $tunnelTask -ErrorAction SilentlyContinue) {
        Stop-ScheduledTask -TaskName $tunnelTask -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $tunnelTask -Confirm:$false
    }
    Get-Process cloudflared -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $cloudflared } | Stop-Process -Force
    Remove-Item -Force $cloudflared, $tunnelToken -ErrorAction SilentlyContinue
}

function Get-File([string]$url, [string]$path) {
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    for ($try = 1; $try -le 3; $try++) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 600 -Uri $url -OutFile $path
            return $true
        } catch {
            Remove-Item -Force $path -ErrorAction SilentlyContinue
            Start-Sleep -Seconds 2
        }
    }
    return $false
}

function Get-Connector([string]$target) {
    $work = Join-Path ([IO.Path]::GetTempPath()) "wirehaus-cf-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Force $work | Out-Null
    try {
        Say 'Descargando el conector del túnel (unos 20 MB)...' 'Downloading the tunnel connector (about 20 MB)...'
        $gz = Join-Path $work 'cloudflared.exe.gz'
        $ok = $false
        if (Get-File "$siteOrigin/download/cloudflared-$cloudflaredVersion-windows-amd64.exe.gz" $gz) {
            if ((Get-FileHash -Algorithm SHA256 $gz).Hash.ToLower() -eq $cloudflaredGzSha256) {
                $inStream = [IO.File]::OpenRead($gz)
                $outStream = [IO.File]::Create($target)
                try {
                    $zip = New-Object IO.Compression.GzipStream($inStream, [IO.Compression.CompressionMode]::Decompress)
                    $zip.CopyTo($outStream)
                } finally {
                    $outStream.Dispose()
                    $inStream.Dispose()
                }
                $ok = (Get-FileHash -Algorithm SHA256 $target).Hash.ToLower() -eq $cloudflaredSha256
            }
        }
        if (-not $ok) {
            Say 'La primera fuente no ha respondido bien; pruebo con la segunda...' 'The first source did not answer well; trying the second one...'
            $url = "https://github.com/cloudflare/cloudflared/releases/download/$cloudflaredVersion/cloudflared-windows-amd64.exe"
            if (-not (Get-File $url $target)) {
                throw 'Could not download the tunnel connector. Check your connection and run the command again.'
            }
            if ((Get-FileHash -Algorithm SHA256 $target).Hash.ToLower() -ne $cloudflaredSha256) {
                Remove-Item -Force $target
                throw 'cloudflared-windows-amd64.exe does not match the expected checksum.'
            }
        }
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}

function Install-Tunnel {
    $download = $ConnectorFile
    if (-not $download) {
        $download = Join-Path ([IO.Path]::GetTempPath()) "cloudflared-$([guid]::NewGuid()).exe"
        Get-Connector $download
    }
    Remove-Tunnel
    Copy-Item -Force $download $cloudflared
    if (-not $ConnectorFile) { Remove-Item -Force $download -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force $dataDir | Out-Null
    (Get-Content -Raw $TunnelTokenFile).Trim() | Set-Content -NoNewline -Encoding ascii $tunnelToken
    $arguments = "--headless `"$cloudflared`" tunnel run --token-file `"$tunnelToken`""
    $action = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument $arguments
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1)
    Register-ScheduledTask -TaskName $tunnelTask -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName $tunnelTask
}

function Test-TunnelUp {
    try {
        $reply = Invoke-WebRequest -UseBasicParsing -TimeoutSec 6 -Uri "https://$PublicHost/link/ping" -Headers @{ Origin = $ViewerOrigin }
        return $reply.StatusCode -eq 200
    } catch {
        return $false
    }
}

function Wait-Tunnel([int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-TunnelUp) { return $true }
        Start-Sleep -Seconds 2
    }
    return $false
}

function Confirm-Tunnel {
    Say 'Comprobando que el túnel conecta...' 'Checking that the tunnel connects...'
    if (Wait-Tunnel 40) {
        Say 'Túnel conectado.' 'Tunnel connected.'
        return
    }
    $mullvad = @("$env:ProgramFiles\Mullvad VPN\resources\mullvad.exe", "$env:ProgramFiles\Mullvad VPN\mullvad.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($mullvad -and ((& $mullvad status 2>$null) -match '^Connected')) {
        Say 'Mullvad VPN bloquea la conexión del túnel. Excluyo solo el conector de Wirehaus de la VPN (el resto de tu tráfico sigue por Mullvad)...' 'Mullvad VPN is blocking the tunnel connection. Excluding only the Wirehaus connector from the VPN (the rest of your traffic still goes through Mullvad)...'
        & $mullvad split-tunnel app add $cloudflared 2>$null | Out-Null
        & $mullvad split-tunnel set on 2>$null | Out-Null
        Stop-ScheduledTask -TaskName $tunnelTask -ErrorAction SilentlyContinue
        Get-Process cloudflared -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $cloudflared } | Stop-Process -Force
        Start-ScheduledTask -TaskName $tunnelTask
        if (Wait-Tunnel 40) {
            Say 'Túnel conectado.' 'Tunnel connected.'
            return
        }
    }
    Say 'Aviso: el túnel no consigue conectar con Cloudflare (puerto 7844), así que tu ordenador aún no es accesible desde fuera.' 'Warning: the tunnel cannot reach Cloudflare (port 7844), so your computer is not reachable from outside yet.'
    Say 'Suele ser una VPN (por ejemplo Mullvad: excluye el conector de la VPN) o el cortafuegos de una red de empresa o colegio. Prueba con otra red o desactiva la VPN un momento; el servicio sigue reintentando solo.' 'It is usually a VPN (for example Mullvad: exclude the connector from the VPN) or the firewall of a company or school network. Try another network or turn the VPN off for a moment; the service keeps retrying by itself.'
}

function Invoke-Agent {
    param([string[]]$Arguments)
    & $agent @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "wirehaus-agent $($Arguments -join ' ') failed with code $LASTEXITCODE"
    }
}

function Test-Package {
    $sums = Join-Path $source 'SHA256SUMS'
    if (-not (Test-Path $sums)) {
        throw 'SHA256SUMS is missing from the package.'
    }
    foreach ($line in Get-Content $sums | Where-Object { $_.Trim() }) {
        $hash, $name = $line -split '\s+', 2
        $path = Join-Path $source ($name.Trim() -replace '/', '\')
        if (-not (Test-Path $path)) {
            throw "$name is listed in SHA256SUMS but missing."
        }
        if ((Get-FileHash -Algorithm SHA256 $path).Hash.ToLower() -ne $hash.ToLower()) {
            throw "$name does not match SHA256SUMS."
        }
    }
}

if ($FetchConnector) {
    Get-Connector $FetchConnector
    exit 0
}

if ($Uninstall) {
    if (Test-Path $agent) {
        Invoke-Agent @('service', 'uninstall')
    }
    Remove-Tunnel
    Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue
    if ($Purge) {
        Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
    }
    Write-Host 'Wirehaus has been removed.'
    exit 0
}

if ($env:WIREHAUS_VERIFIED -eq '1') {
    Get-ChildItem -Recurse -File $source | Unblock-File
}
Test-Package
Say '[1/4] Instalando el agente...' '[1/4] Installing the agent...'

if (Test-Path $agent) {
    try { Invoke-Agent @('service', 'uninstall') } catch { Write-Host 'No previous service to remove.' }
}
New-Item -ItemType Directory -Force $InstallDir | Out-Null
foreach ($file in $files) {
    Copy-Item -Force (Join-Path $source $file) $InstallDir
}
$viewer = Join-Path $InstallDir 'viewer'
Remove-Item -Recurse -Force $viewer -ErrorAction SilentlyContinue
Copy-Item -Recurse -Force (Join-Path $source 'viewer') $viewer

$firstRun = -not (Test-Path $stateFile)
if ($firstRun) {
    Say '[2/4] Segundo factor: escanea este QR con una app de autenticación (2FAS, Aegis, Google Authenticator o Contraseñas de Apple).' '[2/4] Second factor: scan this QR with an authenticator app (2FAS, Aegis, Google Authenticator or Apple Passwords).'
    Invoke-Agent @('init', '--allow-all')
    Write-Host ''
} else {
    Say '[2/4] Este equipo ya estaba configurado (segundo factor incluido). Para activar archivos, terminal, portapapeles y audio de golpe: wirehaus-agent allow-all --code SEIS_DÍGITOS' '[2/4] This computer was already set up (second factor included). To turn on files, terminal, clipboard and audio at once: wirehaus-agent allow-all --code SIX_DIGITS'
}
Say '[3/4] Arrancando el servicio y el túnel...' '[3/4] Starting the service and the tunnel...'

if ($Name -and -not $PublicHost) {
    $PublicHost = "$Name.$Zone"
}
$serviceArguments = @('service', 'install', '--viewer-dir', $viewer)
if ($PublicHost) {
    $serviceArguments += @('--public-host', $PublicHost, '--viewer-origin', $ViewerOrigin)
}
Invoke-Agent $serviceArguments | Out-Null
if ($TunnelTokenFile) {
    Install-Tunnel
    if ($PublicHost) {
        Confirm-Tunnel
    }
}
function Get-DeviceCount {
    $log = Join-Path $dataDir 'state\access.log'
    if (-not (Test-Path $log)) { return 0 }
    return @(Select-String -Path $log -Pattern ' paired ' -SimpleMatch -ErrorAction SilentlyContinue).Count
}

function Wait-Pairing([int]$Before) {
    $deadline = (Get-Date).AddSeconds(600)
    while ((Get-Date) -lt $deadline) {
        if ((Get-DeviceCount) -gt $Before) { return $true }
        Start-Sleep -Seconds 3
    }
    return $false
}

Say '[4/4] Empareja tu móvil:' '[4/4] Pair your phone:'
$devicesBefore = Get-DeviceCount
Invoke-Agent @('pairing')
Write-Host ''
if ($PublicHost) {
    if (-not $TunnelTokenFile) {
        Say 'Antes conecta el túnel (mira tu panel).' 'First connect the tunnel (see your dashboard).'
    }
    $Address = "$ViewerOrigin/$($PublicHost.Split('.')[0])"
    Say "Escanea el QR de arriba con tu móvil (abre $Address con el código puesto) y escribe los seis dígitos de tu app de autenticación." "Scan the QR above with your phone (it opens $Address with the code filled in) and type the six digits from your authenticator app."
    Say 'Esperando a tu móvil... (pulsa Ctrl+C para saltar este paso y hacerlo luego con: wirehaus-agent pairing)' 'Waiting for your phone... (press Ctrl+C to skip this step and do it later with: wirehaus-agent pairing)'
    if (Wait-Pairing $devicesBefore) {
        Say 'Listo: tu móvil está emparejado y conectado. Ya no tienes que tocar nada más en este ordenador.' 'Done: your phone is paired and connected. You do not need to touch anything else on this computer.'
    } else {
        Say 'No he visto ningún móvil emparejarse en 10 minutos. Cuando quieras, ejecuta: wirehaus-agent pairing' 'I did not see a phone pair in 10 minutes. Whenever you like, run: wirehaus-agent pairing'
    }
    Say 'Más adelante, desde el móvil: menú > Vincular otro equipo, para dar acceso a cualquier ordenador escaneando un QR. Para ver otra vez el segundo factor: wirehaus-agent totp' 'Later, from your phone: menu > Link another device, to give any other computer access by scanning a QR. To see the second factor again: wirehaus-agent totp'
} else {
    Write-Host 'Open http://127.0.0.1:7878 on this computer, or reach it through your tunnel, and enter the pairing code.'
}
