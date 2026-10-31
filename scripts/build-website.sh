#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# The website as deployed (see vercel.json): the page, with the install scripts beside it so their addresses are short.
set -eu

cd "$(dirname "$0")/.."
rm -rf site-dist
mkdir site-dist
cp -R website/. site-dist/
cp scripts/install/install.sh scripts/install/install.ps1 site-dist/
