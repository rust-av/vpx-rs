extern crate bindgen;
extern crate system_deps;

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn format_write(builder: bindgen::Builder) -> String {
    builder
        .generate()
        .unwrap()
        .to_string()
        .replace("/**", "/*")
        .replace("/*!", "/*")
}

#[cfg(not(target_env = "msvc"))]
fn try_vcpkg(_statik: bool) -> Option<Vec<PathBuf>> {
    None
}

#[cfg(target_env = "msvc")]
fn try_vcpkg(statik: bool) -> Option<Vec<PathBuf>> {
    if !statik {
        env::set_var("VCPKGRS_DYNAMIC", "1");
    }

    vcpkg::find_package("libvpx")
        .map_err(|e| {
            println!("Could not find ffmpeg with vcpkg: {}", e);
        })
        .map(|library| library.include_paths)
        .ok()
}

fn main() {
    let headers = if let Some(paths) = try_vcpkg(false) {
        paths
    } else {
        let libs = system_deps::Config::new().probe().unwrap();
        let paths = libs.get_by_name("vpx").unwrap().include_paths.clone();
        paths
    };

    let mut builder =
        bindgen::builder()
            .header("data/vpx.h")
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            });

    for header in headers {
        builder = builder.clang_arg("-I").clang_arg(header.to_str().unwrap());
    }

    // Manually fix the comment so rustdoc won't try to pick them
    let s = format_write(builder);

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut file = File::create(out_path.join("vpx.rs")).unwrap();

    let _ = file.write(s.as_bytes());
}
