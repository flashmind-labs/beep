//! Key names to macOS virtual key codes, for the US layout's positions.

/// The code for `name`, as `KeyPress::key` writes it.
pub fn code(name: &str) -> Option<u16> {
    Some(match name {
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "6" => 0x16,
        "5" => 0x17,
        "=" => 0x18,
        "9" => 0x19,
        "7" => 0x1A,
        "-" => 0x1B,
        "8" => 0x1C,
        "0" => 0x1D,
        "]" => 0x1E,
        "o" => 0x1F,
        "u" => 0x20,
        "[" => 0x21,
        "i" => 0x22,
        "p" => 0x23,
        "l" => 0x25,
        "j" => 0x26,
        "'" => 0x27,
        "k" => 0x28,
        ";" => 0x29,
        "\\" => 0x2A,
        "," => 0x2B,
        "/" => 0x2C,
        "n" => 0x2D,
        "m" => 0x2E,
        "." => 0x2F,
        "`" => 0x32,
        // Shift and "=" on a US keyboard.
        "+" => 0x18,
        "return" => 0x24,
        "tab" => 0x30,
        "space" => 0x31,
        "delete" => 0x33,
        "escape" => 0x35,
        "forwarddelete" => 0x75,
        "home" => 0x73,
        "end" => 0x77,
        "pageup" => 0x74,
        "pagedown" => 0x79,
        "left" => 0x7B,
        "right" => 0x7C,
        "down" => 0x7D,
        "up" => 0x7E,
        "f1" => 0x7A,
        "f2" => 0x78,
        "f3" => 0x63,
        "f4" => 0x76,
        "f5" => 0x60,
        "f6" => 0x61,
        "f7" => 0x62,
        "f8" => 0x64,
        "f9" => 0x65,
        "f10" => 0x6D,
        "f11" => 0x67,
        "f12" => 0x6F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_keys_have_codes() {
        assert_eq!(code("t"), Some(0x11));
        assert_eq!(code("return"), Some(0x24));
        assert_eq!(code("left"), Some(0x7B));
        assert_eq!(code("f5"), Some(0x60));
        assert_eq!(code("hyper"), None);
    }

    #[test]
    fn every_letter_and_digit_has_a_code() {
        for c in ('a'..='z').chain('0'..='9') {
            assert!(code(&c.to_string()).is_some(), "{c}");
        }
    }
}
