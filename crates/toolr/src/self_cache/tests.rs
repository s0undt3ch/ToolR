use jiff::{SignedDuration, civil::date, tz::Offset};

use super::human_ago;

#[test]
fn human_ago_picks_the_coarsest_bucket() {
    let now = Offset::UTC
        .to_timestamp(date(2026, 11, 4).at(12, 0, 0, 0))
        .unwrap();
    let cases = [
        (SignedDuration::from_secs(30), "just now"),
        (SignedDuration::from_mins(5), "5m ago"),
        (SignedDuration::from_hours(3), "3h ago"),
        (SignedDuration::from_hours(4 * 24), "4d ago"),
        (SignedDuration::from_hours(90 * 24), "3mo ago"),
        (SignedDuration::from_hours(800 * 24), "2y ago"),
    ];
    for (age, expected) in cases {
        assert_eq!(human_ago(now, now - age), expected, "{age:?}");
    }
}
