use anyhow::{Result, anyhow};
use flac_bound::FlacEncoder;
use metaflac::{Block, BlockType, Tag};
use std::{
    fs::File,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use symphonia::core::{
    audio::{Audio, AudioBuffer},
    codecs::audio::{AudioDecoder, AudioDecoderOptions},
    formats::{FormatOptions, FormatReader, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

struct StreamInfo {
    channels: u32,
    bps: u32,
    sample_rate: u32,
}

pub const CURRENT_VENDOR: &str = "reference libFLAC 1.5.0 20250211";

const BADTAGS: [&str; 3] = ["encoded_by", "encodedby", "encoder"];

fn write_tags(filename: impl AsRef<Path>) -> Result<()> {
    let tags = Tag::read_from_path(&filename)?;
    let temp_name = filename.as_ref().with_extension("tmp");
    let mut output = Tag::read_from_path(&temp_name)?;

    for block in tags.blocks() {
        match block {
            Block::VorbisComment(block) => {
                for (key, val) in block.comments.iter() {
                    if !BADTAGS.contains(&key.to_lowercase().as_str()) {
                        output.set_vorbis(key, val.to_owned());
                    }
                }
            }
            Block::Padding(_) => {}
            _ => output.push_block(block.to_owned()),
        }
    }

    output.write_to_path(temp_name)?;
    Ok(())
}

fn init_decoder(
    file: Box<File>,
) -> Result<(
    u32,
    Box<dyn AudioDecoder>,
    StreamInfo,
    Box<dyn FormatReader>,
)> {
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let mss = MediaSourceStream::new(file, Default::default());
    let fmt_opts = FormatOptions::default();
    let meta_opts: MetadataOptions = Default::default();
    let dec_opts: AudioDecoderOptions = Default::default();

    let probed = symphonia::default::get_probe().probe(&hint, mss, fmt_opts, meta_opts)?;

    let track = probed.default_track(TrackType::Audio).unwrap();

    let codec_params = track.codec_params.as_ref().unwrap().audio().unwrap();

    let decoder = symphonia::default::get_codecs().make_audio_decoder(&codec_params, &dec_opts)?;

    let track_id = track.id;

    let streaminfo = StreamInfo {
        channels: u32::try_from(codec_params.channels.clone().unwrap().count())?,
        bps: codec_params.bits_per_sample.unwrap(),
        sample_rate: codec_params.sample_rate.unwrap(),
    };

    Ok((track_id, decoder, streaminfo, probed))
}

fn encode_file(filename: impl AsRef<Path>, handler: Arc<AtomicBool>) -> Result<bool> {
    let temp_name = filename.as_ref().with_extension("tmp");
    if temp_name.exists() {
        std::fs::remove_file(&temp_name)?;
    }

    let file = Box::new(File::open(&filename)?);

    let (id, mut decoder, streaminfo, mut probe) = init_decoder(file)?;

    let mut encoder = if let Some(encoder) = FlacEncoder::new() {
        if let Ok(encoder) = encoder
            .channels(streaminfo.channels)
            .bits_per_sample(streaminfo.bps)
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

    let mut sample_buf: Option<AudioBuffer<i32>> = None;

    while handler.load(Ordering::SeqCst) {
        let packet = if let Some(packet) = probe.next_packet()? {
            packet
        } else {
            break;
        };

        if packet.track_id() != id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    let spec = audio_buf.spec().clone();

                    sample_buf = Some(AudioBuffer::new(spec, audio_buf.capacity()));
                }

                if let Some(buf) = &mut sample_buf {
                    let buffer = buf
                        .iter_interleaved()
                        .map(|s| {
                            println!("{s} ");
                            s
                        })
                        .collect::<Vec<i32>>();

                    if encoder
                        .process_interleaved(&buffer, u32::try_from(buf.samples_planar()).unwrap())
                        .is_err()
                    {
                        return Err(anyhow!(
                            "Error while processing samples:\t{:?}",
                            encoder.state()
                        ));
                    }
                }
            }
            Err(err) => return Err(err.into()),
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
    let tags = Tag::read_from_path(file)?;
    if let Some(Block::VorbisComment(comment)) = tags.get_blocks(BlockType::VorbisComment).next() {
        Ok(comment.vendor_string.to_owned())
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
