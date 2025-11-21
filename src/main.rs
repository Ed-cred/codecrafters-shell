use std::io::{self, Write};

fn main() {
    let builtins: [&str; 3] = ["exit", "echo", "type"];
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf).unwrap();
        let buf = buf.trim_end();
        let (command, rest) = buf.split_once(' ').unwrap_or((buf, ""));
        match command {
            "exit" => return,
            "echo" => println!("{}", rest),
            "type" => {
                if builtins.contains(&rest) {
                    println!("{rest} is a shell builtin");
                } else {
                    println!("{}: not found", rest);
                }
            }
            _ => println!("{}: command not found", command),
        }
    }
}
