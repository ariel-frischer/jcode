use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "build/docs_manifest.rs"]
mod docs_manifest;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo = manifest.join("../..");
    let docs_manifest_path = manifest.join("runtime-docs.txt");
    println!("cargo:rerun-if-changed={}", docs_manifest_path.display());

    let docs_manifest =
        fs::read_to_string(&docs_manifest_path).expect("read runtime documentation manifest");
    let entries = docs_manifest::parse(&docs_manifest)
        .unwrap_or_else(|error| panic!("invalid runtime documentation manifest: {error}"));
    for entry in &entries {
        println!("cargo:rerun-if-changed={}", repo.join(entry).display());
    }
    let generated = docs_manifest::generate(&repo, &entries)
        .unwrap_or_else(|error| panic!("generate runtime documentation corpus: {error}"));

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("jcode_docs.rs");
    fs::write(out, generated).expect("write generated Jcode documentation corpus");
}
