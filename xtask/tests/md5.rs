//! MD5 against RFC 1321 A.5's test suite, and against Python's `hashlib`
//! across the lengths where the padding moves into a second block.

use xtask::md5::digest;

fn hex(data: &[u8]) -> String {
    digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_rfc_test_suite() {
    let suite: [(&str, &str); 7] = [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ];
    for (message, want) in suite {
        assert_eq!(hex(message.as_bytes()), want, "{message:?}");
    }
}

#[test]
fn the_lengths_where_the_padding_spills() {
    let table = [
        (55, "ef1772b6dff9a122358552954ad0df65"),
        (56, "3b0c8ac703f828b04c6c197006d17218"),
        (63, "b06521f39153d618550606be297466d5"),
        (64, "014842d480b571495a4a0363793f7367"),
        (65, "c743a45e0d2e6a95cb859adae0248435"),
        (119, "8a7bd0732ed6a28ce75f6dabc90e1613"),
        (120, "5f61c0ccad4cac44c75ff505e1f1e537"),
    ];
    for (n, want) in table {
        assert_eq!(hex(&vec![b'a'; n]), want, "{n} bytes");
    }
}

#[test]
fn many_blocks() {
    let bytes: Vec<u8> = (0..1280)
        .map(|i: usize| u8::try_from(i % 256).expect("a byte"))
        .collect();
    assert_eq!(hex(&bytes), "82829f1f3f2bb0f18b25f278e5bba8bd");
}
