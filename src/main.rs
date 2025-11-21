use std::io::{self, Write};

fn main() {
    print!("$ ");
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    match io::stdin().read_line(&mut buf) {
        Ok(_) => {
            let command = buf.trim_end();
            println!("{}: command not found", command)
        }
        Err(e) => println!("error {e}"),
    }
}
