use crate::markdown::style_sheet::{CatppuccinStyleSheet, StyleSheet};

#[derive(Debug, Clone)]
#[non_exhaustive]
pub(crate) struct Options<S: StyleSheet = CatppuccinStyleSheet> {
    pub(crate) styles: S,
}

impl<S: StyleSheet> Options<S> {
    pub(crate) const fn new(styles: S) -> Self {
        Self { styles }
    }
}

impl Default for Options<CatppuccinStyleSheet> {
    fn default() -> Self {
        Self::new(CatppuccinStyleSheet)
    }
}
