mod typewriter;
mod mandir;
mod common;

use std::io::Read;
use anyhow::Result;

fn main() -> Result<()> {
    // let mut input = String::new();
    // std::io::stdin().read_to_string(&mut input)?;
    // let mut t = typewriter::Typewriter::new();
    // for c in input.chars() {
    //     t.append(c);
    // }
    // println!("{}", t.to_html(false));

    let cat = jmclib::dirs::rootpath("share/manual.toml")?;
    let mut md = mandir::Mandir::new(&cat, "/usr/bin/mandoc")?;
    md.add_mandir("/usr/share/man")?;
    println!("mandir: {:#?}", md);

    println!("index: {:#?}", md.index()?);
    // println!("section 1M: {:#?}", md.pages("1M"));

    Ok(())
}
