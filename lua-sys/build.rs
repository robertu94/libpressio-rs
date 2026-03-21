fn main() {
    let artifacts = lua_src::Build::new().build(lua_src::Lua54);
    artifacts.print_cargo_metadata();
    println!(
        "cargo:root={}",
        artifacts.include_dir().parent().unwrap().display()
    );
    println!("cargo:include={}", artifacts.include_dir().display());
}
