//! Símbolos y radios covalentes (fuente en `docs/data-sources.md`).

const SYMBOLS: [&str; 118] = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl",
    "Ar", "K", "Ca", "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As",
    "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr", "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In",
    "Sn", "Sb", "Te", "I", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd", "Pm", "Sm", "Eu", "Gd", "Tb",
    "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf", "Ta", "W", "Re", "Os", "Ir", "Pt", "Au", "Hg", "Tl",
    "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "U", "Np", "Pu", "Am", "Cm", "Bk",
    "Cf", "Es", "Fm", "Md", "No", "Lr", "Rf", "Db", "Sg", "Bh", "Hs", "Mt", "Ds", "Rg", "Cn", "Nh",
    "Fl", "Mc", "Lv", "Ts", "Og",
];

/// Radios covalentes en Å para Z = 1..=96 (Cordero et al., 2008). Para C, Mn, Fe y Co se usa el
/// valor sp3 / bajo espín.
const COVALENT_RADII: [f64; 96] = [
    0.31, 0.28, 1.28, 0.96, 0.84, 0.76, 0.71, 0.66, 0.57, 0.58, 1.66, 1.41, 1.21, 1.11, 1.07, 1.05,
    1.02, 1.06, 2.03, 1.76, 1.70, 1.60, 1.53, 1.39, 1.39, 1.32, 1.26, 1.24, 1.32, 1.22, 1.22, 1.20,
    1.19, 1.20, 1.20, 1.16, 2.20, 1.95, 1.90, 1.75, 1.64, 1.54, 1.47, 1.46, 1.42, 1.39, 1.45, 1.44,
    1.42, 1.39, 1.39, 1.38, 1.39, 1.40, 2.44, 2.15, 2.07, 2.04, 2.03, 2.01, 1.99, 1.98, 1.98, 1.96,
    1.94, 1.92, 1.92, 1.89, 1.90, 1.87, 1.87, 1.75, 1.70, 1.62, 1.51, 1.44, 1.41, 1.36, 1.36, 1.32,
    1.45, 1.46, 1.48, 1.40, 1.50, 1.50, 2.60, 2.21, 2.15, 2.06, 2.00, 1.96, 1.90, 1.87, 1.80, 1.69,
];

/// Radio para elementos sin dato tabulado (Z > 96).
const DEFAULT_RADIUS: f64 = 1.5;

/// Símbolo del elemento `z` (1..=118).
pub fn symbol(z: u8) -> &'static str {
    SYMBOLS
        .get(usize::from(z).wrapping_sub(1))
        .copied()
        .unwrap_or("?")
}

/// Radio covalente en Å.
pub fn covalent_radius(z: u8) -> f64 {
    COVALENT_RADII
        .get(usize::from(z).wrapping_sub(1))
        .copied()
        .unwrap_or(DEFAULT_RADIUS)
}

/// No metales y metaloides (H, He, B, C, N, O, F, Ne, Si, P, S, Cl, Ar, Ge, As, Se, Br, Kr, Sb,
/// Te, I, Xe, At, Rn); el resto se considera metal.
pub fn is_metal(z: u8) -> bool {
    !matches!(
        z,
        1 | 2 | 5..=10 | 14..=18 | 32..=36 | 51..=54 | 85 | 86
    )
}

/// Número atómico a partir de un símbolo (`Ti`, `TI`, `ti`), de un símbolo con sufijo (`Ti1`, `O_2`)
/// o de un número atómico (`22`).
pub fn parse_element(token: &str) -> Option<u8> {
    if let Ok(z) = token.parse::<u8>() {
        return (1..=118).contains(&z).then_some(z);
    }
    let end = token
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(token.len());
    let letters = &token[..end];
    if !(1..=2).contains(&letters.len()) {
        return None;
    }
    SYMBOLS
        .iter()
        .position(|e| e.eq_ignore_ascii_case(letters))
        .map(|i| i as u8 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_and_numbers() {
        assert_eq!(parse_element("Ti"), Some(22));
        assert_eq!(parse_element("TI"), Some(22));
        assert_eq!(parse_element("al"), Some(13));
        assert_eq!(parse_element("O"), Some(8));
        assert_eq!(parse_element("Co"), Some(27));
        assert_eq!(parse_element("C1"), Some(6));
        assert_eq!(parse_element("Ti12"), Some(22));
        assert_eq!(parse_element("8"), Some(8));
        assert_eq!(parse_element("0"), None);
        assert_eq!(parse_element("119"), None);
        assert_eq!(parse_element("Xx"), None);
        assert_eq!(parse_element(""), None);
        assert_eq!(symbol(22), "Ti");
        assert_eq!(symbol(0), "?");
        assert_eq!(covalent_radius(8), 0.66);
        assert_eq!(covalent_radius(118), DEFAULT_RADIUS);
        assert!(is_metal(22) && is_metal(13) && is_metal(79));
        assert!(!is_metal(8) && !is_metal(1) && !is_metal(14) && !is_metal(86));
    }
}
