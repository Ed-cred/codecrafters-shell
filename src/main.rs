use std::io::{self, Write};

fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();
        let mut buf = String::new();
        match io::stdin().read_line(&mut buf) {
            Ok(_) => {
                let input = buf.trim_end();
                let (command, rest) = input.split_once(' ').unwrap_or((input, ""));
                match command {
                    "exit" => return,
                    "echo" => println!("{}", rest),
                    _ => println!("{}: command not found", command),
                }
            }
            Err(e) => println!("error {e}"),
        }
    }
}
