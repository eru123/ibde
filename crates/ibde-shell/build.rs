fn main() {
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    // no libpam.so dev symlink on this host: link the soname via an
    // explicit search (filename form, not -l shorthand)
    println!("cargo:rustc-link-arg=-l:libpam.so.0");
}
