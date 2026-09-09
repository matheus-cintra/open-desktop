use std::path::PathBuf;

use opendesk_core::uri_list::{parse_uri_list, to_uri_list};

#[test]
fn parses_crlf_separated_file_uris_and_skips_comments() {
    let text = "# comment\r\nfile:///home/user/a.txt\r\n\r\nfile://localhost/tmp/b\nhttp://example.com/c\r\n";
    assert_eq!(
        parse_uri_list(text),
        vec![PathBuf::from("/home/user/a.txt"), PathBuf::from("/tmp/b")]
    );
}

#[test]
fn ignores_non_local_hosts_and_relative_paths() {
    assert!(parse_uri_list("file://otherhost/tmp/b\nfile://relative\nfile:").is_empty());
}

#[test]
fn percent_decodes_paths() {
    assert_eq!(
        parse_uri_list("file:///home/user/my%20file%25.txt\n"),
        vec![PathBuf::from("/home/user/my file%.txt")]
    );
    assert_eq!(
        parse_uri_list("file:///bad%2\n"),
        vec![PathBuf::from("/bad%2")]
    );
}

#[test]
fn encodes_reserved_characters_and_ends_lines_with_crlf() {
    let paths = vec![
        PathBuf::from("/home/user/my file.txt"),
        PathBuf::from("/tmp/a#b?c&d_-.~"),
    ];
    assert_eq!(
        to_uri_list(&paths),
        "file:///home/user/my%20file.txt\r\nfile:///tmp/a%23b%3Fc%26d_-.~\r\n"
    );
    assert_eq!(to_uri_list(&[]), "");
}

#[test]
fn round_trips_unicode_and_special_characters() {
    let paths = vec![
        PathBuf::from("/home/user/relatório final.pdf"),
        PathBuf::from("/tmp/100%/x+y"),
    ];
    assert_eq!(parse_uri_list(&to_uri_list(&paths)), paths);
}
