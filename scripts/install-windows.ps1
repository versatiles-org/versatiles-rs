# PowerShell script to install versatiles on Windows

# Function to detect the system architecture
function Detect-Architecture {
   $arch = (Get-WmiObject Win32_OperatingSystem).OSArchitecture
   if ($arch -eq "64-bit") {
      $cpuArch = (Get-WmiObject Win32_Processor).Architecture
      if ($cpuArch -eq 9) {
         return "x86_64"
      } elseif ($cpuArch -eq 12) {
         return "aarch64"
      } else {
         Write-Host "Unsupported CPU architecture: $cpuArch" -ForegroundColor Red
         exit 1
      }
   } else {
      Write-Host "Unsupported OS architecture: $arch" -ForegroundColor Red
      exit 1
   }
}

# Function to download and install the package
function Install-Package {
   param (
      [string]$architecture
   )

   $packageUrl = "https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-windows-$architecture.tar.gz"
   $downloadPath = "$env:TEMP\versatiles.tar.gz"
   $installDir = "$env:ProgramFiles\versatiles"

   # Download the package
   Write-Host "Downloading versatiles for $architecture..." -ForegroundColor Green
   Invoke-WebRequest -Uri $packageUrl -OutFile $downloadPath
   if (-not $?) {
      Write-Host "Failed to download the package." -ForegroundColor Red
      exit 1
   }

   # Create installation directory if it doesn't exist
   if (-not (Test-Path $installDir)) {
      New-Item -ItemType Directory -Path $installDir
   }

   # Extract the tar.gz file
   Write-Host "Extracting the package..." -ForegroundColor Green
   tar -xzf $downloadPath -C $installDir
   if (-not $?) {
      Write-Host "Failed to extract the package." -ForegroundColor Red
      exit 1
   }

   # Add the directory to the PATH if not already included
   if (-not $env:Path.Contains($installDir)) {
      Write-Host "Adding $installDir to system PATH..." -ForegroundColor Green
      $env:Path += ";$installDir"
      [Environment]::SetEnvironmentVariable("Path", $env:Path, [EnvironmentVariableTarget]::Machine)

      # Reload the PATH in the current session
      $env:Path = [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::Machine)
   }

   # Clean up
   Remove-Item $downloadPath

   Write-Host "VersaTiles installed successfully." -ForegroundColor Green
}

# Main script execution
$architecture = Detect-Architecture
Install-Package -architecture $architecture
