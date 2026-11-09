    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("ключ".as_bytes()), "0LrQu9GO0Yc=");
    }

    #[test]
    fn a_wrapped_line_counts_every_row_it_takes() {
        assert_eq!(wrapped_height("", 10), 1);
        assert_eq!(wrapped_height("abcdefghij", 10), 1);
        assert_eq!(wrapped_height("abcdefghijk", 10), 2);
        assert_eq!(wrapped_height("ab\ncd", 10), 2);
        // Cyrillic is one column per letter, not one per byte.
        assert_eq!(wrapped_height("ключключкл", 10), 1);
    }
