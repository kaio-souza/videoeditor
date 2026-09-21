# ALSA dev headers (vendored)

`include/` and `lib/pkgconfig/alsa.pc` here are the **build-time-only**
headers and pkg-config metadata from the `libasound2-dev` package
(alsa-lib project, https://www.alsa-project.org/), licensed **LGPL-2.1**.
No compiled/runtime library (`libasound.so.2` or similar) is vendored —
`lib/libasound.so` is a tiny linker script (`GROUP ( libasound.so.2 )`,
not ELF) that redirects the linker to whatever ALSA runtime library is
already installed on the build machine at its normal system location.

## Why this exists

`alsa-sys` (pulled in by `rodio`/`cpal` for audio playback, seção 12 of
CLAUDE.md) needs `alsa.pc` + `asoundlib.h` to succeed its `pkg-config`
probe at build time — this is a **build-time-only** requirement, nothing
here ships inside the compiled binary or is needed at runtime. Most Linux
distros don't install `libasound2-dev` by default, and the app must build
without asking anyone to run `sudo apt-get install ...` first (see
CLAUDE.md — "Áudio"). Vendoring these small header/pkg-config files (redistribution
allowed under LGPL-2.1) makes `cargo build` self-sufficient on any Linux
machine with the ALSA *runtime* library present (near-universal on Linux
desktops — part of `libasound2`, and ALSA-API calls are commonly routed
through PulseAudio/PipeWire's ALSA-compat layer even on systems not
running ALSA directly).

Not needed and not used on Windows/macOS — `alsa-sys` is a Linux-only
dependency (gated by `cpal`'s own `Cargo.toml`); `.cargo/config.toml`'s
extra `PKG_CONFIG_PATH` entry is simply unused there.

Source: `libasound2-dev 1.2.11-1ubuntu0.3` (Ubuntu noble-security),
extracted via `apt-get download libasound2-dev` (no root needed) +
`dpkg-deb -x`.
