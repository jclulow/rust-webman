mod typewriter;

use std::io::Read;

fn main() -> std::io::Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut t = typewriter::Typewriter::new();
    for c in input.chars() {
        t.append(c);
    }
    println!("{}", t.to_html(false));
    Ok(())
}
