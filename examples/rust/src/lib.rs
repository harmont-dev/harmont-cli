pub fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::pos(2, 3, 5)]
    #[case::neg(-1, -1, -2)]
    #[case::zero(0, 0, 0)]
    fn adds(#[case] a: i64, #[case] b: i64, #[case] expected: i64) {
        assert_eq!(add(a, b), expected);
    }
}
