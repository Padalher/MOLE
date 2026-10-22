# Changelog
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [SemVer](https://semver.org/).

## [0.2.1] — 2026-10-22
### Fixed
- macOS (XQuartz): `mole` could not open the window ("cannot connect to the X display: No such file or directory"). The `DISPLAY` path that XQuartz sets (`/private/tmp/com.apple.launchd.…/org.xquartz:0`) was ignored by x11rb 0.14, which tried `/tmp/.X11-unix/X0` instead; MOLE now connects to that path.

## [0.2.0] — 2026-10-21
First public release.

### Added
- Viewer `mole <file>...`, a window that contains:
  - a 3D ball-and-stick view (rotate, move, zoom, hover an atom);
  - a static, clickable E vs n plot (hover a point to see n, E and the label);
  - playback controls (first, previous, play/pause, next, last, speed, loop);
  - bond tolerance and metal–metal bonds controls;
  - keyboard shortcuts.
- Drawing with the core X11 protocol (x11rb), so that the window stays fluid over `ssh -Y`.
- `--smooth` mode (also a checkbox in the window), with antialiased spheres and bonds through XRender.
- Readers:
  - multi-frame XYZ files with the energy in the comment line, including ORCA `*_trj.xyz` trajectories;
  - Gaussian 16 outputs (last geometry and its `SCF Done`);
  - ORCA 6.1 outputs (last geometry and its `FINAL SINGLE POINT ENERGY`).
- `mole --list`: tab-separated structure table on stdout, without opening a window.
- Banner when `mole` is run without arguments.
- `MOLE_BENCH=n`: plays `n` frames, prints the frame rate and exits.
- Example: optimization trajectory of a water molecule (`examples/`).
- Distribution:
  - Python wheels (`pip install mole-viewer`) for Linux, macOS and Windows;
  - a static Linux x86_64 binary.
