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

    let docs_manifest = match fs::read_to_string(&docs_manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => fail(format!("read runtime documentation manifest: {error}")),
    };
    let entries = match docs_manifest::parse(&docs_manifest) {
        Ok(entries) => entries,
        Err(error) => fail(format!("invalid runtime documentation manifest: {error}")),
    };
    for entry in &entries {
        println!("cargo:rerun-if-changed={}", repo.join(entry).display());
    }
    let generated = match docs_manifest::generate(&repo, &entries) {
        Ok(generated) => generated,
        Err(error) => fail(format!("generate runtime documentation corpus: {error}")),
    };

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("jcode_docs.rs");
    fs::write(out, generated).expect("write generated Jcode documentation corpus");
}

fn fail(message: String) -> ! {
    eprintln!("jcode-app-core build error: {message}");
    std::process::exit(1)
}
