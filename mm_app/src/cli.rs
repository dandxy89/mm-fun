/// Parses the trading symbol from command-line arguments
pub fn get_symbol(default: &str, uppercase: bool) -> String {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        let symbol = &args[1];
        if uppercase { symbol.to_uppercase() } else { symbol.to_lowercase() }
    } else {
        default.to_string()
    }
}

/// Parses the trading symbol from command-line arguments (uppercase version)
pub fn get_symbol_uppercase(default: &str) -> String {
    get_symbol(default, true)
}

/// Parses the trading symbol from command-line arguments (lowercase version)
pub fn get_symbol_lowercase(default: &str) -> String {
    get_symbol(default, false)
}
