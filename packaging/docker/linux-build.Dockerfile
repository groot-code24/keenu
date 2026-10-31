# Builds Keenu for Linux with everything its packages hold: GTK 4 for the shelf, whisper.cpp for speech.
#
#   docker build -t keenu-linux-build -f packaging/docker/linux-build.Dockerfile packaging/docker
#   docker run --rm -v "$PWD:/src" -v keenu-linux-target:/target -w /src keenu-linux-build scripts/package.sh
FROM rust:1-trixie
RUN apt-get update \
 && apt-get install -y --no-install-recommends libgtk-4-dev cmake g++ clang libclang-dev libvulkan-dev glslc \
      dpkg-dev rpm file shellcheck \
 && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_DIR=target-linux
