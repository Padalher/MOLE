use mole_core::{EnergySource, load};
use std::path::PathBuf;

/// Ejemplo incluido en el repositorio (ORCA 6.1.1, `examples/water_opt.inp`).
#[test]
fn water_example() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/water_opt_trj.xyz");
    let l = load(&[path]);
    assert!(l.errors.is_empty(), "{:?}", l.errors);
    assert_eq!(l.structures.len(), 9);
    assert_eq!(l.structures[0].energy, Some(-76.227362292694));
    assert_eq!(l.structures[8].energy, Some(-76.321838583357));
    assert_eq!(l.structures[8].formula(), "H2O");
}

/// Archivos reales propios (no versionados): `MOLE_SAMPLES=/ruta/a/ejemplos cargo test`.
#[test]
fn real_samples() {
    let Ok(dir) = std::env::var("MOLE_SAMPLES") else {
        eprintln!("MOLE_SAMPLES no definido: prueba con archivos reales omitida");
        return;
    };
    let dir = PathBuf::from(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!(!files.is_empty());
    for f in &files {
        let l = load(std::slice::from_ref(f));
        assert!(l.errors.is_empty(), "{}: {:?}", f.display(), l.errors);
        assert!(!l.structures.is_empty());
        assert!(l.structures.iter().all(|s| !s.atoms.is_empty()));
        eprintln!(
            "{}: {} estructuras, {} avisos",
            f.display(),
            l.structures.len(),
            l.warnings.len()
        );
    }
}

#[test]
fn xyz_without_energy_takes_it_from_matching_gaussian_output() {
    let dir = std::env::temp_dir().join(format!("mole-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let xyz = dir.join("conjunto.xyz");
    std::fs::write(&xyz, "1\nlabel=a1\nH 0 0 0\n1\nlabel=b2\nH 0 0 0\n").unwrap();
    let out = " Entering Gaussian System\n Standard orientation:\n -----\n h\n h\n -----\n 1 1 0 0.0 0.0 0.0\n -----\n SCF Done:  E(RHF) =  -0.5  A.U.\n Normal termination of Gaussian 16\n";
    std::fs::write(dir.join("calc_a1.out"), out).unwrap();
    let l = load(&[xyz]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(l.structures[0].energy, Some(-0.5));
    assert_eq!(
        l.structures[0].energy_source,
        Some(EnergySource::GaussianScf)
    );
    assert_eq!(l.structures[1].energy, None);
    assert!(l.warnings.iter().any(|w| w.contains("1 of 2")));
}
