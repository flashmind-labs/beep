use super::*;

#[test]
fn memory_length_counts_characters_not_bytes() {
    assert!(check_memory(&"é".repeat(4000)).is_ok());
    assert!(check_memory(&"é".repeat(4001)).is_err());
    assert!(check_memory(" ").is_err());
}
