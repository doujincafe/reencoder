use anyhow::{Result, anyhow};
use claxon::{FlacReader, FlacReaderOptions};
use flac_bound::FlacEncoder;
use metaflac::{Block, Tag};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub const CURRENT_VENDOR: &str = "reference libFLAC 1.5.0 20250211";

fn write_tags(filename: impl AsRef<Path>) -> Result<()> {
    let tags = Tag::read_from_path(&filename)?;
    let temp_name = filename.as_ref().with_extension("tmp");
    let mut output = Tag::read_from_path(&temp_name)?;

    if let Some(streaminfo) = tags.get_streaminfo() {
        output.set_streaminfo(streaminfo.clone());
    }

    for block in tags.blocks() {
        match block {
            Block::VorbisComment(comment) => {
                for (key, val) in comment.comments.clone() {
                    if key.to_lowercase() != "encoder" || key.to_lowercase() != "encoded by" {
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

fn encode_file(filename: impl AsRef<Path>, handler: Arc<AtomicBool>) -> Result<bool> {
    let temp_name = filename.as_ref().with_extension("tmp");
    if temp_name.exists() {
        std::fs::remove_file(&temp_name)?;
    }
    let mut decoder = FlacReader::open(&filename)?;
    let streaminfo = decoder.streaminfo();

    let num_channels: usize = streaminfo.channels.try_into()?;

    let mut encoder = if let Some(encoder) = FlacEncoder::new() {
        if let Ok(encoder) = encoder
            .channels(streaminfo.channels)
            .bits_per_sample(streaminfo.bits_per_sample)
            .sample_rate(streaminfo.sample_rate)
            .compression_level(8)
            .verify(false)
            .init_file(&temp_name)
        {
            encoder
        } else {
            return Err(anyhow!("failed to create encoder"));
        }
    } else {
        return Err(anyhow!("failed to create encoder"));
    };

    let mut frame_reader = decoder.blocks();
    let mut buffer = Vec::new();
    let mut block_buffer =
        Vec::with_capacity(streaminfo.max_block_size as usize * num_channels as usize);

    loop {
        if !handler.load(Ordering::SeqCst) {
            let _ = encoder.finish();
            std::fs::remove_file(temp_name)?;
            return Ok(true);
        }
        match frame_reader.read_next_or_eof(block_buffer) {
            Ok(Some(block)) => {
                for sample in 0..(block.len() / block.channels()) {
                    for ch in 0..block.channels() {
                        buffer.push(block.sample(ch, sample));
                    }
                }

                if let Err(_) = encoder.process_interleaved(&buffer, block.len() / block.channels())
                {
                    return Err(anyhow!(
                        "Error while processing samples:\t{:?}",
                        encoder.state()
                    ));
                };
                buffer.clear();
                block_buffer = block.into_buffer();
            }
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        }
    }

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }
    write_tags(&filename)?;
    std::fs::rename(temp_name, filename)?;
    Ok(false)
}

pub fn handle_encode(filename: impl AsRef<Path>, handler: Arc<AtomicBool>) -> Result<bool> {
    match encode_file(&filename, handler) {
        Err(error) => {
            let _ = std::fs::remove_file(filename.as_ref().with_extension("tmp"));
            Err(error)
        }
        Ok(res) => Ok(res),
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
        let name = "./samples/16bit.flac";
        let tempname = "./samples/16bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }

    #[test]
    fn bit24() {
        let name = "./samples/24bit.flac";
        let tempname = "./samples/24bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }

    #[test]
    fn bit32() {
        let name = "./samples/32bit.flac";
        let tempname = "./samples/32bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        let handler = Arc::new(AtomicBool::new(true));
        encode_file(name, handler).unwrap();
        let output = std::process::Command::new("flac")
            .arg("-wts")
            .arg(name)
            .status();
        std::fs::rename(tempname, name).unwrap();
        assert!(output.unwrap().success());
    }
}
