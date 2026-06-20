use chrono::Duration;
use ttask::time::parse_duration;

#[test]
fn duration_10min_roundtrip() {
    let d = parse_duration("10min").unwrap();
    assert_eq!(d, Duration::minutes(10));
}

#[test]
fn duration_2h_roundtrip() {
    let d = parse_duration("2h").unwrap();
    assert_eq!(d, Duration::hours(2));
}

#[test]
fn duration_negative_rejected() {
    assert!(parse_duration("-1h").is_err());
}
