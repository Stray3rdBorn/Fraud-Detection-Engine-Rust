// turns an id of any length into a 64 bit number, using the FNV 1a method, the result is the same on every run.
pub fn id_hash(id: &str) -> u64 {
    // the starting value defined by the FNV 1a method.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    // mix in every byte of the id.
    for byte in id.bytes() {
        // combine the byte with the current value.
        hash ^= u64::from(byte);
        // spread the change over all bits, wrapping_mul ignores overflow on purpose.
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // the final number.
    hash
}

// tests for the hash.
#[cfg(test)]
mod tests {
    // bring the function above into the tests.
    use super::*;

    // two published FNV 1a values and the property that matters, equal ids give equal numbers.
    #[test]
    fn id_hash_matches_known_values() {
        // the empty id gives the starting value.
        assert_eq!(id_hash(""), 0xcbf2_9ce4_8422_2325);
        // the published value for the id a.
        assert_eq!(id_hash("a"), 0xaf63_dc4c_8601_ec8c);
        // different ids give different numbers.
        assert_ne!(id_hash("u1"), id_hash("u2"));
    }
}
