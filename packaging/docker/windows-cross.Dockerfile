# Builds Keenu for Windows from Linux with mingw-w64, for contributors without Visual Studio, and makes its
# installers with msitools and NSIS. Official releases are built with MSVC by .github/workflows/release.yml.
#
#   docker build -t keenu-windows-cross -f packaging/docker/windows-cross.Dockerfile packaging/docker
#   docker run --rm -v "$PWD:/src" -v keenu-cross-target:/target -w /src keenu-windows-cross \
#     scripts/windows/package.sh
FROM rust:1-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 \
      wixl nsis zip \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add x86_64-pc-windows-gnu \
 && rustup component add rustfmt clippy
ENV CARGO_TARGET_DIR=/target
