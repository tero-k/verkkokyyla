use verkkokyyla_lib::engine::trace_parse::{parse_traceroute_line, parse_tracert_line, RawHop};

#[test]
fn parse_tracert_line_handles_happy_path_and_less_than_one_ms_variants() {
    assert_eq!(
        parse_tracert_line("  1    <1 ms    <1 ms    <1 ms  192.168.1.1"),
        Some(RawHop {
            hop: 1,
            address: Some("192.168.1.1".to_owned()),
            rtts: vec![Some(0.5), Some(0.5), Some(0.5)],
            annotation: None,
        })
    );

    assert_eq!(
        parse_tracert_line("  4    < 1 ms    12 ms    <1 ms  10.0.0.4"),
        Some(RawHop {
            hop: 4,
            address: Some("10.0.0.4".to_owned()),
            rtts: vec![Some(0.5), Some(12.0), Some(0.5)],
            annotation: None,
        })
    );
}

#[test]
fn parse_tracert_line_handles_all_timeout_and_mixed_rtts() {
    assert_eq!(
        parse_tracert_line("  2     *        *        *     Request timed out."),
        Some(RawHop {
            hop: 2,
            address: None,
            rtts: vec![None, None, None],
            annotation: None,
        })
    );

    assert_eq!(
        parse_tracert_line("  3    12 ms    *      11 ms  10.0.0.1"),
        Some(RawHop {
            hop: 3,
            address: Some("10.0.0.1".to_owned()),
            rtts: vec![Some(12.0), None, Some(11.0)],
            annotation: None,
        })
    );
}

#[test]
fn parse_tracert_line_returns_none_for_localized_headers_and_garbage() {
    assert_eq!(
        parse_tracert_line("Reitin jäljitys kohteeseen example.com"),
        None
    );
    assert_eq!(parse_tracert_line("Routenverfolgung zu example.com"), None);
    assert_eq!(parse_tracert_line("  7    こんにちは世界    ✨"), None);
}

#[test]
fn parse_traceroute_line_handles_happy_path_and_star_hop() {
    assert_eq!(
        parse_traceroute_line(" 1  192.168.1.1  0.537 ms"),
        Some(RawHop {
            hop: 1,
            address: Some("192.168.1.1".to_owned()),
            rtts: vec![Some(0.537)],
            annotation: None,
        })
    );

    assert_eq!(
        parse_traceroute_line(" 2  *"),
        Some(RawHop {
            hop: 2,
            address: None,
            rtts: vec![None],
            annotation: None,
        })
    );
}

#[test]
fn parse_traceroute_line_handles_annotation_and_multi_address_hops() {
    assert_eq!(
        parse_traceroute_line(" 4  10.0.0.1  5 ms !H"),
        Some(RawHop {
            hop: 4,
            address: Some("10.0.0.1".to_owned()),
            rtts: vec![Some(5.0)],
            annotation: Some("!H".to_owned()),
        })
    );

    assert_eq!(
        parse_traceroute_line(" 5  10.0.0.1  8 ms  10.0.0.2  9 ms  10.0.0.3  10 ms"),
        Some(RawHop {
            hop: 5,
            address: Some("10.0.0.1".to_owned()),
            rtts: vec![Some(8.0), Some(9.0), Some(10.0)],
            annotation: None,
        })
    );
}
