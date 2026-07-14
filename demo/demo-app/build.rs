fn main() {
    rust_executable_metadata::PackageMetadataBuilder::new_from_cargo("example")
        .expect("Should be able to parse cargo environment variables")
        .maintainer("you@example.com")
        .copyright("2026 EXAMPLE COMPANY")
        .build()
        .unwrap()
        .write_linker_script_and_inject_argument()
        .unwrap();
}
