use anyhow::{Result, anyhow};
use claxon::{FlacReader, FlacReaderOptions};
use flac_bound::FlacEncoder;
use md5::{Digest, Md5};
use metaflac::{Block, Tag};
use std::path::Path;

pub const CURRENT_VENDOR: &str = "reference libFLAC 1.5.0 20250211";

fn write_tags(filename: impl AsRef<Path>, hash: Vec<u8>) -> Result<()> {
    let tags = Tag::read_from_path(&filename)?;
    let temp_name = filename.as_ref().with_extension("tmp");
    let mut output = Tag::read_from_path(&temp_name)?;

    let mut streaminfo = tags.get_streaminfo().unwrap().clone();

    streaminfo.md5 = hash;
    output.set_streaminfo(streaminfo);

    for block in tags.blocks() {
        match block {
            Block::VorbisComment(comment) => {
                for (key, val) in comment.comments.clone() {
                    if key != "ENCODER" {
                        output.set_vorbis(key, val);
                    }
                }
            }
            Block::StreamInfo(_) | Block::Padding(_) => {}
            _ => output.push_block(block.clone()),
        }
    }

    output.write_to_path(temp_name)?;
    Ok(())
}

fn encode_file(filename: impl AsRef<Path>) -> Result<()> {
    let temp_name = filename.as_ref().with_extension("tmp");
    if temp_name.exists() {
        std::fs::remove_file(&temp_name)?;
    }
    let mut decoder = FlacReader::open(&filename)?;
    let streaminfo = decoder.streaminfo();

    let mut encoder = FlacEncoder::new()
        .unwrap()
        .channels(streaminfo.channels)
        .bits_per_sample(streaminfo.bits_per_sample)
        .sample_rate(streaminfo.sample_rate)
        .compression_level(8)
        .verify(false)
        .init_file(&temp_name)
        .unwrap();

    let mut hasher = Md5::new();

    let samples = decoder
        .samples()
        .map(|res| {
            let sample = res.unwrap();
            match streaminfo.bits_per_sample {
                16 => {
                    hasher.update(i16::try_from(sample).unwrap().to_le_bytes());
                }
                24 => {
                    hasher.update(i24::i24::try_from_i32(sample).unwrap().to_le_bytes());
                }
                32 => {
                    hasher.update(sample.to_le_bytes());
                }
                _ => {}
            }
            sample
        })
        .collect::<Vec<i32>>();

    encoder
        .process_interleaved(&samples, samples.len() as u32 / streaminfo.channels)
        .unwrap();

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    let hash = hasher.finalize().to_vec();
    write_tags(&filename, hash)?;
    std::fs::rename(temp_name, filename)?;
    Ok(())
}

pub fn handle_encode(filename: impl AsRef<Path>) -> Result<()> {
    if let Err(error) = encode_file(&filename) {
        let _ = std::fs::remove_file(filename.as_ref().with_extension("tmp"));
        Err(error)
    } else {
        Ok(())
    }
}

pub fn get_vendor(file: impl AsRef<Path>) -> Result<String> {
    if let Some(vendor) = FlacReader::open_ext(
        file,
        FlacReaderOptions {
            metadata_only: true,
            read_vorbis_comment: true,
        },
    )?
    .vendor()
    {
        Ok(vendor.to_string())
    } else {
        Err(anyhow!("Vendor string not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit16() {
        let name = "16bit.flac";
        let tempname = "16bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = FlacReader::open(tempname).unwrap().streaminfo().md5sum;
        let temp_md5 = FlacReader::open(name).unwrap().streaminfo().md5sum;

        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, temp_md5);
    }

    #[test]
    fn bit24() {
        let name = "24bit.flac";
        let tempname = "24bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = FlacReader::open(tempname).unwrap().streaminfo().md5sum;
        let temp_md5 = FlacReader::open(name).unwrap().streaminfo().md5sum;

        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, temp_md5);
    }

    #[test]
    fn bit32() {
        let name = "32bit.flac";
        let tempname = "32bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = FlacReader::open(tempname).unwrap().streaminfo().md5sum;
        let temp_md5 = FlacReader::open(name).unwrap().streaminfo().md5sum;

        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, temp_md5);
    }
}
