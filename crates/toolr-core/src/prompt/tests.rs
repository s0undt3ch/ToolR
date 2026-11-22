use super::*;

fn ask(input: &str, default: bool) -> (bool, String) {
    let mut out = Vec::new();
    let answer = yes_no(&mut std::io::Cursor::new(input), &mut out, "Go?", default);
    (answer, String::from_utf8(out).unwrap())
}

#[test]
fn accepts_y_and_yes_in_any_case() {
    for input in ["y\n", "Y\n", "yes\n", "YES\n", " yes \n"] {
        assert!(ask(input, false).0, "{input:?}");
    }
}

#[test]
fn declines_n_no_and_anything_else() {
    for input in ["n\n", "no\n", "nope\n", "ye\n", "I\n"] {
        assert!(!ask(input, true).0, "{input:?}");
    }
}

#[test]
fn bare_enter_takes_the_default_and_the_hint_shows_it() {
    let (answer, out) = ask("\n", false);
    assert!(!answer);
    assert_eq!(out, "Go? [y/N] ");
    let (answer, out) = ask("\n", true);
    assert!(answer);
    assert_eq!(out, "Go? [Y/n] ");
}

#[test]
fn eof_declines_even_by_default_and_ends_the_line() {
    let (answer, out) = ask("", true);
    assert!(!answer);
    assert_eq!(out, "Go? [Y/n] \n");
}

#[test]
fn a_read_error_declines() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
    }
    let mut out = Vec::new();
    let mut input = std::io::BufReader::new(Broken);
    assert!(!yes_no(&mut input, &mut out, "Go?", true));
}
