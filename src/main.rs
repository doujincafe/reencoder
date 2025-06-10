mod db;
mod flac;

fn main() {
    if let Err(err) = flac::encode_file(std::path::Path::new("32bit.flac")) {
        println!("{}", err)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaflac::Tag;

    #[test]
    fn bit16() {
        std::fs::copy("16bit.flac", "16bit.flac.temp").unwrap();
        flac::encode_file(std::path::Path::new("16bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("16bit.flac.temp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path("16bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file("16bit.flac.temp").unwrap();
        assert_eq!(target_md5, encoded_md5);
    }

    #[test]
    fn bit24() {
        std::fs::copy("24bit.flac", "24bit.flac.temp").unwrap();
        flac::encode_file(std::path::Path::new("24bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("24bit.flac.temp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path("24bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file("24bit.flac.temp").unwrap();
        assert_eq!(target_md5, encoded_md5);
    }

    #[test]
    fn bit32() {
        std::fs::copy("32bit.flac", "32bit.flac.temp").unwrap();
        flac::encode_file(std::path::Path::new("32bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("32bit.flac.temp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path("32bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file("32bit.flac.temp").unwrap();
        assert_eq!(target_md5, encoded_md5);
    }
}
