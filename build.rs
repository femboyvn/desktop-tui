fn main() {
    println!("cargo:rerun-if-changed=src/runtime/audio_output.c");
    println!("cargo:rerun-if-changed=vendor/miniaudio/miniaudio.h");
    cc::Build::new().file("src/runtime/audio_output.c").include("vendor/miniaudio")
        .warnings(false).compile("desktop_audio");
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    }
}
