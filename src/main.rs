mod flac;

fn main() {
    if let Err(error) = flac::encode_file(std::path::Path::new("./1.flac")) {
        println!("{}", error)
    };
}
