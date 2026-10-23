# MOLE

*[English version](README.md)*

MOLE es un visor ligero de conjuntos de estructuras moleculares: búsquedas de confórmeros, corridas de optimización global y trayectorias de optimización de geometría. Se lanza desde la terminal y abre una ventana con:

- **la estructura en 3D**: bolas y barras; se gira, se mueve y se hace zoom con el ratón;
- **la gráfica de energía E vs n**: al pasar el ratón sobre un punto se ven n, E y la etiqueta; un clic muestra esa estructura;
- **controles de reproducción**: primera, anterior, reproducir/pausa, siguiente, última, velocidad y repetición.

MOLE está pensado para **correr en una máquina remota (un clúster HPC o una estación de trabajo) y verse por `ssh -Y`**. Dibuja con el protocolo X11 básico, así que por la red solo viajan órdenes de dibujo (unos KB por fotograma) y la ventana va fluida incluso con una conexión doméstica. Solo lee resultados; nunca ejecuta cálculos.

## Instalación
MOLE es un único ejecutable sin dependencias. No hacen falta permisos de administrador.

**Con pip** (Linux x86_64/aarch64, macOS, Windows):
```bash
pip install mole-viewer        # o: pip install --user mole-viewer · pipx install mole-viewer · uv tool install mole-viewer
mole                           # el paquete se llama mole-viewer; el comando es mole
```

Si no hay `pip` (algo común en los clústeres, donde no se pueden instalar paquetes del sistema), usa un entorno virtual, que trae su propio pip:
```bash
python3 -m venv ~/.local/mole-viewer
~/.local/mole-viewer/bin/pip install mole-viewer
mkdir -p ~/.local/bin && ln -sf ~/.local/mole-viewer/bin/mole ~/.local/bin/mole
```
(`~/.local/bin` debe estar en tu `PATH`; si no, añade `export PATH="$HOME/.local/bin:$PATH"` a `~/.bashrc`.)

**Binario precompilado** (Linux x86_64, estático; funciona con cualquier glibc):
```bash
curl -LO https://github.com/Padalher/MOLE/releases/latest/download/mole-x86_64-linux
chmod +x mole-x86_64-linux
mkdir -p ~/.local/bin && mv mole-x86_64-linux ~/.local/bin/mole
```

**Desde el código fuente** (requiere [Rust](https://rustup.rs)):
```bash
cargo install --git https://github.com/Padalher/MOLE mole
```

## Ver MOLE desde tu computadora
Instala MOLE donde están tus archivos (por ejemplo, el clúster) y conéctate con reenvío X11:
```bash
ssh -Y usuario@cluster
cd /ruta/a/resultados
mole estructuras.xyz
```

Tu computadora necesita un servidor X para mostrar la ventana:

| Tu computadora | Servidor X |
|---|---|
| Linux | incluido (X11 o Xwayland) |
| macOS | [XQuartz](https://www.xquartz.org) (cierra sesión y vuelve a entrar tras instalarlo) |
| Windows | [MobaXterm](https://mobaxterm.mobatek.net) (servidor X y cliente SSH en uno), o [VcXsrv](https://github.com/marchaesen/vcxsrv) con cualquier cliente SSH |

MOLE también corre en local en Linux. En macOS y Windows corre en local a través de XQuartz o VcXsrv (en Windows, `set DISPLAY=localhost:0`), aunque esto todavía no se ha probado.

## Uso
```bash
mole conjunto.xyz                 # XYZ con una o varias estructuras
mole calc_*.out                   # salidas de Gaussian u ORCA: una estructura por archivo
mole opt_trj.xyz                  # trayectoria de optimización de ORCA (E vs paso)
mole a.xyz b.xyz                  # varios archivos, en el orden dado
mole --smooth conjunto.xyz        # dibujo suavizado (XRender)
mole --list conjunto.xyz          # tabla de estructuras en la terminal, sin ventana
mole --help
```

- **Teclado:** ← → anterior/siguiente · espacio reproducir/pausa · Inicio/Fin primera/última · q o Esc salir.
- **Ratón (vista 3D):** arrastrar gira · botón derecho mueve · rueda acerca · doble clic restablece.

La interfaz está en inglés.

**Pruébalo** con el ejemplo incluido en este repositorio: la optimización de geometría de una molécula de agua distorsionada (ORCA 6.1.1, B3LYP-D3BJ/def2-SVP; la entrada está en [`examples/water_opt.inp`](examples/water_opt.inp)):
```bash
mole examples/water_opt_trj.xyz
```

### Formatos y energías
| Archivo | Estructuras | Energía |
|---|---|---|
| `.xyz` | todas | de la línea de comentario de cada una: un número al inicio (`-3219847.92  etiqueta`), `E=-76.32`, `energy=-76.32` o `… E -76.32` (trayectorias `*_trj.xyz` de ORCA) |
| salida de Gaussian (`.out`, `.log`) | última geometría | última `SCF Done` |
| salida de ORCA (`.out`, `.log`) | última geometría | su `FINAL SINGLE POINT ENERGY` |

El programa que generó el archivo se reconoce por su contenido, no por la extensión. Las energías se muestran tal como se leen, sin convertir unidades. Las estructuras sin energía se pueden ver, pero no aparecen en la gráfica.

Probado con archivos XYZ y con salidas de Gaussian 16 Rev. B.01 y de ORCA 6.1.

## Cómo citarlo
Si usas MOLE en un trabajo publicado, por favor cítalo. Los datos están en [`CITATION.cff`](CITATION.cff), y el botón *Cite this repository* de GitHub los convierte a APA o BibTeX.

## Autores
- Pablo D. Alvarez-Hernandez: Universidad Juárez Autónoma de Tabasco, DACB – CICTAT, México.
- Filiberto Ortiz Chi: Cinvestav Unidad Mérida, TheoChem Mérida, México.

MOLE fue diseñado, dirigido y validado por Pablo D. Alvarez-Hernandez. Se usaron asistentes de programación basados en IA para escribir y reestructurar partes del código, las pruebas automáticas y la documentación; cada cambio fue revisado y validado por el autor. Las energías y geometrías que muestra MOLE siempre se leen de los archivos de salida de los cálculos de química cuántica; nunca las genera un modelo de lenguaje.

## Licencia
[MIT](LICENSE).
