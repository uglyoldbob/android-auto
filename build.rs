fn main() {
    let out_dir_env = std::env::var_os("OUT_DIR").unwrap();
    let out_dir = std::path::Path::new(&out_dir_env);
    protobuf_codegen::Codegen::new()
        .out_dir(out_dir)
        .protoc()
        .includes(&["protobuf"])
        .input("protobuf/Bluetooth.proto")
        .input("protobuf/Wifi.proto")
        .cargo_out_dir("protobuf")
        .run_from_script();
}
