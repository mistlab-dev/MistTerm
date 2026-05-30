fn main() {
    println!("cargo:rerun-if-changed=assets/fonts/NotoSansSC-Regular.otf");
    println!("cargo:rerun-if-changed=build.rs");
}
