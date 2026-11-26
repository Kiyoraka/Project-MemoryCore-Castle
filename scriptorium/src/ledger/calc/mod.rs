//! Ledger's calculation engine: formulas worked out by our own evaluator. The calc lane owns this
//! folder; the rest of Ledger only calls [`recalc`] after a change.
//!
//! Until the engine lands, nothing is calculated and the cached values stay as the file had them.

use super::model::Sheet;

/// Works out every formula in `sheets` again and writes the results into the cells. Answers how many
/// cells changed value (0 while nothing is calculated yet).
pub fn recalc(sheets: &mut [Sheet]) -> usize {
    let _ = sheets;
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn nothing_is_calculated_yet() {
        assert_eq!(super::recalc(&mut []), 0);
    }
}
