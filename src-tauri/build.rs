fn main() {
    // macOS: Info.plist (the microphone line) is read when the app is compiled; rebuild when it changes.
    println!("cargo:rerun-if-changed=Info.plist");
    // DEV > Phone: the Android emulator's own gRPC contract, compiled with a vendored protoc so no
    // member or CI runner has to install one. Client only - Castle never serves it.
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("the vendored protoc is missing");
    std::env::set_var("PROTOC", protoc);
    let include = protoc_bin_vendored::include_path().expect("the vendored protobuf includes are missing");
    tonic_build::configure()
        .build_server(false)
        .compile_protos(&["proto/emulator_controller.proto"], &[std::path::PathBuf::from("proto"), include])
        .expect("the emulator's proto could not be compiled");
    println!("cargo:rerun-if-changed=proto/emulator_controller.proto");
    tauri_build::build()
}
