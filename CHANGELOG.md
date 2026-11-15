# Changelog
Format: [Keep a Changelog](https://keepachangelog.com/). Versioning: [SemVer](https://semver.org/).

## [Unreleased]
### Added
- Gaussian cube files (`.cube`, `.cub`): atoms and isosurfaces, with the positive lobe in red and the negative one in blue, an isovalue control and an optional *Transparent* mode. Several cube files can be browsed like structures. Multi-orbital files show the first data set, with a warning.
- White theme: *Light theme* checkbox and `--light`.
- The controls column can be widened by dragging its left edge.
- Extended XYZ: the numeric per-atom columns named in `Properties=` are read; hovering over an atom shows their values, and a `forces:R:3` column is drawn as arrows (scaled per structure, *Arrows* checkbox).

### Changed
- The controls column is grouped under titles: Playback, Bonds, Surface (cube files), Forces (files with forces), View and Plot.
- New command-line options for every control: `--frame N`, `--absolute`, `--fix-orientation`, `--tol T`, `--no-mm`, `--iso V`, `--transparent` and `--no-forces` (see `mole --help`).
- README, `CITATION.cff` and package description: MOLE is described by the files it opens, not by particular kinds of calculation.
- Contact addresses of the authors in the README, `CITATION.cff` and the package metadata; questions about the software: contact@padalher.com.
- README: tested on Linux, macOS (XQuartz) and Windows through WSL, locally and over ssh.
- With fewer than two energies, the E vs n plot opens at its minimum height; a single energy no longer gives an axis of 10^-9 steps.
- Very small energy differences: the axis moves the power of ten to its label (`E - Emin (×10^-6)`) and the tooltip uses exponents (`1.23e-6`) instead of `0.000001`.

## [0.3.0] — 2026-11-04
### Changed
- The controls are in a single column at the top right, next to the 3D view, one per row: the playback buttons are larger and labeled (First, Last, Play/Pause, Previous, Next), so the mouse travels less between the structure and the buttons. The 3D view and the E vs n plot share the rest of the window.
- Metal–metal bonds are drawn by default, so pure metal clusters (Cu, Ag, Au…) show their bonds when opened; the *M-M bonds* checkbox turns them off.
- Every element has its own color: elements without a MOLE-specific color use the Blue Obelisk Data Repository colors (MIT license).

### Added
- MOLE icon (`assets/mole.svg`); the window sets it (`_NET_WM_ICON`), so docks and taskbars show it instead of a generic icon, also over `ssh -Y`.
- Dragging with the left button on the E vs n plot browses the structures; the plot itself still does not move or zoom.
- *Fix orientation* checkbox: the structure on screen becomes the reference, and every other structure is turned to the same orientation. Structures with the same atoms in the same order (trajectories, conformers) are superposed atom by atom; isomers are aligned by their principal axes, choosing the orientation that best matches the reference.
- Dragging the top edge of the E vs n plot makes it taller or shorter; the 3D view takes the rest of the height, and the chosen height is kept when the window is resized.
- The E axis shows relative energies, E − Emin, by default (in the units of the file, with the value of Emin above the plot), so large values such as `-3219847.95` no longer give long axis labels; the *E - Emin* checkbox switches the axis to the absolute energies as read. The header and the tooltip always show each energy as read.

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
