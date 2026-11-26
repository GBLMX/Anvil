pub fn greeting(name: &str) -> String {
    format!("hello, {name}")
}

pub fn farewell(name: &str) -> String {
    format!("goodbye, {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets() {
        assert_eq!(greeting("Anvil"), "hello, Anvil");
    }

    #[test]
    fn says_goodbye() {
        assert_eq!(farewell("Anvil"), "goodbye, Anvil");
    }
}
