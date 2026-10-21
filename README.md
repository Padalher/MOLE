# MOLE

*[Versión en español](https://github.com/Padalher/MOLE/blob/main/README.es.md)*

MOLE is a lightweight viewer for ensembles of molecular structures (conformer searches, global optimization runs, geometry optimization trajectories). Launched from the terminal, it opens a window with:

- **the 3D structure**: ball and stick; rotate, move and zoom with the mouse;
- **the energy plot E vs n**: hover a point to see n, E and the label; click it to show that structure;
- **playback controls**: first, previous, play/pause, next, last, speed and loop.

MOLE is designed to run **on a remote machine (an HPC cluster or a workstation) and be viewed over `ssh -Y`**. It draws with the core X11 protocol, so only drawing commands travel over the network (a few KB per frame), and the window stays fluid even on a home connection. It only reads results; it never runs calculations.

## Installation
MOLE is a single executable with no runtime dependencies. No administrator rights are needed.

**With pip** (Linux x86_64/aarch64, macOS, Windows):
```bash
pip install mole-viewer        # or: pip install --user mole-viewer · pipx install mole-viewer · uv tool install mole-viewer
mole                           # the package is called mole-viewer; the command is mole
```

**Prebuilt binary** (Linux x86_64, static; works with any glibc):
```bash
curl -LO https://github.com/Padalher/MOLE/releases/latest/download/mole-x86_64-linux
chmod +x mole-x86_64-linux
mkdir -p ~/.local/bin && mv mole-x86_64-linux ~/.local/bin/mole
```

**From source** (requires [Rust](https://rustup.rs)):
```bash
cargo install --git https://github.com/Padalher/MOLE mole
```

## Viewing MOLE from your computer
Install MOLE where your files are (e.g., the cluster) and connect with X11 forwarding:
```bash
ssh -Y user@cluster
cd /path/to/results
mole structures.xyz
```

Your computer needs an X server to display the window:

| Your computer | X server |
|---|---|
| Linux | built in (X11 or Xwayland) |
| macOS | [XQuartz](https://www.xquartz.org) (log out and back in after installing it) |
| Windows | [MobaXterm](https://mobaxterm.mobatek.net) (X server and SSH client in one), or [VcXsrv](https://github.com/marchaesen/vcxsrv) with any SSH client |

MOLE also runs locally on Linux. On macOS and Windows, it runs locally through XQuartz or VcXsrv (`set DISPLAY=localhost:0` on Windows), although this has not been tested yet.

## Usage
```bash
mole ensemble.xyz                 # XYZ file with one or more structures
mole calc_*.out                   # Gaussian or ORCA outputs: one structure per file
mole opt_trj.xyz                  # ORCA optimization trajectory (E vs step)
mole a.xyz b.xyz                  # several files, in the given order
mole --smooth ensemble.xyz        # antialiased drawing (XRender)
mole --list ensemble.xyz          # print the structure table, no window
mole --help
```

- **Keyboard:** ← → previous/next · space play/pause · Home/End first/last · q or Esc quit.
- **Mouse (3D view):** drag to rotate · right button to move · wheel to zoom · double click to reset.

**Try it** with the example included in this repository: the geometry optimization of a distorted water molecule (ORCA 6.1.1, B3LYP-D3BJ/def2-SVP, input in [`examples/water_opt.inp`](https://github.com/Padalher/MOLE/blob/main/examples/water_opt.inp)):
```bash
mole examples/water_opt_trj.xyz
```

### Formats and energies
| File | Structures | Energy |
|---|---|---|
| `.xyz` | all frames | from each comment line: a leading number (`-3219847.92  label`), `E=-76.32`, `energy=-76.32` or `… E -76.32` (ORCA `*_trj.xyz`) |
| Gaussian output (`.out`, `.log`) | last geometry | last `SCF Done` |
| ORCA output (`.out`, `.log`) | last geometry | its `FINAL SINGLE POINT ENERGY` |

The program is recognized by the file contents, not by the extension. Energies are shown as read, without unit conversion. Structures without an energy can be viewed, but they are not plotted.

Tested with XYZ files, Gaussian 16 Rev. B.01 and ORCA 6.1 outputs.

## Citation
If you use MOLE in published work, please cite it. The metadata are in [`CITATION.cff`](https://github.com/Padalher/MOLE/blob/main/CITATION.cff), and GitHub's *Cite this repository* button turns them into APA or BibTeX.

## Authors
- Pablo D. Alvarez-Hernandez: Universidad Juárez Autónoma de Tabasco, DACB – CICTAT, Mexico.
- Filiberto Ortiz Chi: Cinvestav Unidad Mérida, TheoChem Mérida, Mexico.

MOLE was developed with the assistance of [Claude Code](https://claude.com/claude-code) (Anthropic).

## License
[MIT](https://github.com/Padalher/MOLE/blob/main/LICENSE).

## Development
```bash
cargo test                                   # tests
cargo clippy --all-targets && cargo deny check
MOLE_SAMPLES=/path/to/files cargo test       # also read your own files
MOLE_BENCH=200 mole file.xyz                 # play 200 frames, print the frame rate and exit
```

`mole-core` reads the files and has no graphics dependencies; `mole` is the window (X11 via [x11rb](https://github.com/psychon/x11rb)). Releases are built and published by GitHub Actions when a `v*` tag is pushed.
