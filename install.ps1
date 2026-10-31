# SPDX-License-Identifier: GPL-3.0-or-later
# Installs Keenu on Windows from an official release: the same script as the website serves, for a copy of this repository.
#
#   .\install.ps1              install the newest release
#   .\install.ps1 -Uninstall   remove it; your data stays
& (Join-Path $PSScriptRoot 'scripts\install\install.ps1') @args
exit $LASTEXITCODE
