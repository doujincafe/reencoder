use anyhow::{Result, anyhow};
use flac_bound::FlacEncoder;
use flac_codec::{
    decode::{Metadata, verify},
    *,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) const CURRENT_VENDOR: &str = "reference libFLAC 1.5.0 20250211";
const BADTAGS: [&str; 3] = ["encoded_by", "encodedby", "encoder"];

fn encode_file(filename: &Path, handler: Arc<AtomicBool>) -> Result<bool> {
    if verify(filename).is_err() {
        return Err(anyhow!("corrupt file"));
    };

    let temp_name = filename.with_extension("tmp");
    if temp_name.exists() {
        std::fs::remove_file(&temp_name)?;
    }

    let mut reader = decode::FlacSampleReader::open(filename)?;

    let blocklist = reader.metadata();

    let streaminfo = blocklist.streaminfo();

    let channels = streaminfo.channel_count() as u32;

    let metadata = blocklist
        .blocks()
        .filter_map(|block| {
            use metadata::Block;
            use metadata::BlockRef::*;
            match block {
                SeekTable(table) => Some(Block::SeekTable(table.clone())),
                Application(app) => Some(Block::Application(app.clone())),
                Cuesheet(sheet) => Some(Block::Cuesheet(sheet.clone())),
                Picture(picture) => Some(Block::Picture(picture.clone())),
                VorbisComment(comments) => {
                    let mut cloned = comments.clone();
                    for tag in BADTAGS {
                        cloned.remove(tag);
                    }
                    cloned.vendor_string = CURRENT_VENDOR.to_string();
                    Some(Block::VorbisComment(cloned))
                }
                _ => None,
            }
        })
        .collect::<Vec<metadata::Block>>();

    let mut encoder = if let Some(encoder) = FlacEncoder::new() {
        if let Ok(encoder) = {
            let mut encoder = encoder
                .channels(streaminfo.channel_count() as u32)
                .bits_per_sample(streaminfo.bits_per_sample())
                .sample_rate(streaminfo.sample_rate())
                .compression_level(8)
                .verify(false);
            if let Some(size) = reader.total_samples() {
                encoder = encoder.total_samples_estimate(size)
            }
            encoder.init_file(&temp_name)
        } {
            encoder
        } else {
            return Err(anyhow!("failed to create encoder"));
        }
    } else {
        return Err(anyhow!("failed to create encoder"));
    };

    while handler.load(Ordering::SeqCst) {
        match reader.fill_buf() {
            Ok(buf) => {
                if !buf.is_empty() {
                    let length = buf.len();
                    if encoder
                        .process_interleaved(buf, length as u32 / channels)
                        .is_err()
                    {
                        return Err(anyhow!(
                            "Error while processing samples:\t{:?}",
                            encoder.state()
                        ));
                    };

                    reader.consume(length);
                } else {
                    break;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }

    if !handler.load(Ordering::SeqCst) {
        let _ = encoder.finish();
        std::fs::remove_file(temp_name)?;
        return Ok(true);
    }

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    metadata::update(&temp_name, |blocklist| {
        for block in metadata {
            use metadata::Block::*;
            match block {
                Application(b) => {
                    let _ = blocklist.insert(b);
                }
                Picture(b) => {
                    let _ = blocklist.insert(b);
                }
                VorbisComment(b) => {
                    let _ = blocklist.insert(b);
                }
                Cuesheet(b) => {
                    let _ = blocklist.insert(b);
                }
                SeekTable(b) => {
                    let _ = blocklist.insert(b);
                }
                _ => {}
            }
        }
        Ok::<(), flac_codec::Error>(())
    })?;

    std::fs::rename(&temp_name, filename)?;

    Ok(false)
}

pub fn handle_encode(filename: &Path, handler: Arc<AtomicBool>) -> Result<bool> {
    match encode_file(filename, handler) {
        Err(error) => {
            let _ = std::fs::remove_file(filename.with_extension("tmp"));
            Err(error)
        }
        Ok(res) => Ok(res),
    }
}

pub fn get_vendor(file: &Path) -> Result<String> {
    let blocklist = metadata::BlockList::open(file)?;
    if let Some(data) = blocklist.get::<metadata::VorbisComment>() {
        Ok(data.vendor_string.to_owned())
    } else {
        Err(anyhow!("Vendor string not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bit16() {
        let name = PathBuf::from("./samples/16bit.flac");
        let tempname = PathBuf::from("./samples/16bit.flac.temp");
        std::fs::copy(&name, &tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(&name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(&name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }

    #[test]
    fn bit24() {
        let name = PathBuf::from("./samples/24bit.flac");
        let tempname = PathBuf::from("./samples/24bit.flac.temp");
        std::fs::copy(&name, &tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(&name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(&name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }

    #[test]
    fn bit32() {
        let name = PathBuf::from("./samples/32bit.flac");
        let tempname = PathBuf::from("./samples/32bit.flac.temp");
        std::fs::copy(&name, &tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(&name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(&name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }
}
