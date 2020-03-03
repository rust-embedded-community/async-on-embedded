use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = &PathBuf::from(env::var("OUT_DIR")?);

    // place the linker script somewhere the linker can find it
    let filename = "memory.x";
    fs::copy(filename, out_dir.join(filename))?;
    println!("cargo:rustc-link-search={}", out_dir.display());

    Ok(())
}
