use std::io::{self, Write};

fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();
        let mut buf = String::new();
        match io::stdin().read_line(&mut buf) {
            Ok(_) => {
                let command = buf.trim_end();
                if command == "exit" {
                    return;
                }
                println!("{}: command not found", command)
            }
            Err(e) => println!("error {e}"),
        }
    }
}
