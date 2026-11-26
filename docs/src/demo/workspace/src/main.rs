use std::io;

fn main() {
    let name = "Anvil";
    println!("hello from {name}");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("read greeting");
}
