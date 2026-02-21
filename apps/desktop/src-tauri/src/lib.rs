pub fn placeholder_engine_value(value: i32) -> i32 {
    value + 1
}

#[cfg(test)]
mod tests {
    use super::placeholder_engine_value;

    #[test]
    fn increments_placeholder_value() {
        assert_eq!(placeholder_engine_value(41), 42);
    }
}
