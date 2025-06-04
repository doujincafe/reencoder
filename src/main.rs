mod flac;

fn main() {
    flac::encode_file(std::path::Path::new("./1.flac")).unwrap();
}
