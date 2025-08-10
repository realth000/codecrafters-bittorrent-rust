use serde_json::{self, Number};
use std::env;

fn decode_bencoded_value(encoded_value: &str) -> serde_json::Value {
    let flag = encoded_value.chars().next().unwrap();
    if flag.is_digit(10) {
        // String "5:hello" -> "hello"
        let colon_index = encoded_value.find(':').unwrap();
        let number_string = &encoded_value[..colon_index];
        let number = number_string.parse::<usize>().unwrap();
        let string = &encoded_value[colon_index + 1..colon_index + 1 + number];
        return serde_json::Value::String(string.to_string());
    } else if flag == 'i' {
        // Interger "i52e" -> 52; "i-52e" -> -52
        let interger_end_pos = encoded_value.find('e').unwrap();
        let number: isize = encoded_value[1..interger_end_pos].parse().unwrap();
        return serde_json::Value::Number(Number::from(number));
    } else {
        panic!("Unhandled encoded value: {}", encoded_value)
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("no args specified");
        return;
    }
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];
        let decoded_value = decode_bencoded_value(encoded_value);
        println!("{}", decoded_value.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }
}
