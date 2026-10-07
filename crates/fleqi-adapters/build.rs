fn main() {
    println!("cargo:rerun-if-changed=src/terminal/bash_bridge.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("fleqi-bash.so");
    let compiler = cc::Build::new().get_compiler();
    let status = compiler
        .to_command()
        .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg("src/terminal/bash_bridge.c")
        .arg("-o")
        .arg(output)
        .arg("-ldl")
        .status()
        .expect("start compiler for Bash integration");
    assert!(status.success(), "Bash integration compilation failed");
}
