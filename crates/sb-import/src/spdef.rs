//! The default sprite definitions (SPDEF), which SmileBASIC 4 keeps in its
//! executable rather than in a file.

use sb_runtime::sprites::{DEF_BYTES, DEFS, FIRST_RELEASE_DEFS, defs_from_bytes};

/// Finds SmileBASIC 4's built-in template table in its executable's
/// read-only data: it starts with 16x16 cells at (0,0) and (16,0). Returns
/// the table's bytes. The first release has only the first
/// [`FIRST_RELEASE_DEFS`] templates; the rest are left empty, as unused
/// templates are in later releases.
pub fn find_default_defs(ro: &[u8]) -> Option<Vec<u8>> {
    let start: Vec<u8> = [0u16, 0, 16, 16, 0, 0, 0, 16, 0, 16, 16, 0, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let starts: Vec<usize> = (0..ro.len())
        .filter(|&at| ro[at..].starts_with(&start))
        .collect();
    for count in [DEFS, FIRST_RELEASE_DEFS] {
        for &at in &starts {
            let Some(table) = ro.get(at..at + count * DEF_BYTES) else {
                continue;
            };
            let mut table = table.to_vec();
            table.resize(DEFS * DEF_BYTES, 0);
            if defs_from_bytes(&table).is_some() {
                return Some(table);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::sprites::SpriteDef;

    #[test]
    fn default_table_is_found() {
        let mut bytes = vec![0xAB; 10];
        let table: Vec<u8> = (0..DEFS as u16)
            .flat_map(|n| [n % 128 * 16, n / 128 * 16, 16, 16, 0, 0, 0])
            .flat_map(|v| v.to_le_bytes())
            .collect();
        bytes.extend_from_slice(&table);
        assert_eq!(find_default_defs(&bytes), Some(table.clone()));
        // A first-release table, followed by other data.
        let mut first = bytes[..10 + FIRST_RELEASE_DEFS * DEF_BYTES].to_vec();
        first.extend_from_slice(&[0xFF; 1000]);
        let found = find_default_defs(&first).expect("table");
        assert_eq!(
            found[..FIRST_RELEASE_DEFS * DEF_BYTES],
            table[..FIRST_RELEASE_DEFS * DEF_BYTES]
        );
        let defs = defs_from_bytes(&found).unwrap();
        assert_eq!(defs[FIRST_RELEASE_DEFS], SpriteDef::default());
        assert_eq!(find_default_defs(&[0xAB; 100]), None);
    }
}
