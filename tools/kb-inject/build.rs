fn main() {
    // macros write files at compile time of the build script
    println!("cargo:rerun-if-changed=protocol/virtual-keyboard-unstable-v1.xml");
}
