# Builds CordFast for Linux (x86_64), in a container with the system
# libraries it links: ALSA for audio, Wayland/X11 for windows, fontconfig
# for system fonts, libopus (found by pkg-config, linked statically).
# To run, a desktop also needs libxkbcommon-x11, ALSA and OpenGL (Mesa).
FROM rust:1.98-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake pkg-config clang libopus-dev \
        libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
        libxrandr-dev libxi-dev libgl1-mesa-dev libfontconfig1-dev libfreetype6-dev \
        libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*
# .cargo/config.toml names the Windows Build Tools' CMake; a variable set
# here wins over it.
ENV CMAKE=cmake
WORKDIR /src
CMD ["cargo", "build", "--release", "--target-dir", "target-linux"]
