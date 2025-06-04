mod flac;

fn main() {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaflac::Tag;
    #[test]
    fn bit16() {
        flac::encode_file(std::path::Path::new("16bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("16bit.flac.tmp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let source_md5 = Tag::read_from_path("16bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(std::path::Path::new("16bit.flac.tmp")).unwrap();
        assert_eq!(target_md5, source_md5);
    }
    #[test]
    fn bit24() {
        flac::encode_file(std::path::Path::new("24bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("24bit.flac.tmp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let source_md5 = Tag::read_from_path("24bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(std::path::Path::new("24bit.flac.tmp")).unwrap();
        assert_eq!(target_md5, source_md5);
    }
    #[test]
    fn bit32() {
        flac::encode_file(std::path::Path::new("32bit.flac")).unwrap();
        let target_md5 = Tag::read_from_path("32bit.flac.tmp")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let source_md5 = Tag::read_from_path("32bit.flac")
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(std::path::Path::new("32bit.flac.tmp")).unwrap();
        assert_eq!(target_md5, source_md5);
    }
}
