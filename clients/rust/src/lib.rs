#[cfg(test)]
mod tests {
    #[test]
    fn test_bonka_value() {
        // Test the Value enum
        let int_value = bonka::Value::Int(42);
        let string_value = bonka::Value::String("Hello".to_string());
        let float_value = bonka::Value::Float(13.37);
        let bool_value = bonka::Value::Bool(true);

        assert_eq!(int_value, bonka::Value::Int(42));
        assert_eq!(string_value, bonka::Value::String("Hello".to_string()));
        assert_eq!(float_value, bonka::Value::Float(13.37));
        assert_eq!(bool_value, bonka::Value::Bool(true));
    }
}
