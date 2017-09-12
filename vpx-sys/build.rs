extern crate bindgen;
extern crate metadeps;

use std::fs::OpenOptions;
use std::io::Write;

fn format_write(builder: bindgen::Builder, output: &str) {
    let s = builder.generate()
        .unwrap()
        .to_string()
        .replace("/**", "/*")
        .replace("/*!", "/*");

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(output)
        .unwrap();

    let _ = file.write(s.as_bytes());
}

fn common_builder() -> bindgen::Builder {
    bindgen::builder()
        .raw_line("#![allow(dead_code)]")
        .raw_line("#![allow(non_camel_case_types)]")
        .raw_line("#![allow(non_snake_case)]")
        .raw_line("#![allow(non_upper_case_globals)]")
}

fn main() {
    let libs = metadeps::probe().unwrap();
    let headers = libs.get("vpx").unwrap().include_paths.clone();
    // let buildver = libs.get("vpx").unwrap().version.split(".").nth(1).unwrap();

    let mut builder = common_builder()
        .header("data/vpx.h");

    for header in headers {
        builder = builder.clang_arg("-I").clang_arg(header.to_str().unwrap());
    }

    // Manually fix the comment so rustdoc won't try to pick them
    format_write(builder, "src/vpx.rs");
}
