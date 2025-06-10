use anyhow::{Result, anyhow};
use flac_bound::{FlacEncoder, WriteWrapper};
use i24::i24;
use md5::{Digest, Md5, Md5Core, digest::core_api::CoreWrapper};
use metaflac::{Block, Tag};
use std::fs::File;
use symphonia::core::{
    audio::{Audio, GenericAudioBufferRef},
    codecs::audio::AudioDecoder,
    formats::{FormatOptions, FormatReader, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

struct StreamConfig {
    channels: u32,
    bits_per_sample: Bps,
    sample_rate: u32,
}

enum Bps {
    _16,
    _24,
    _32,
}

impl Bps {
    fn new(num: u32) -> Result<Self> {
        match num {
            16 => Ok(Bps::_16),
            24 => Ok(Bps::_24),
            32 => Ok(Bps::_32),
            _ => Err(anyhow!("Invalid BPS")),
        }
    }

    fn value(&self) -> u32 {
        match self {
            Bps::_16 => 16,
            Bps::_24 => 24,
            Bps::_32 => 32,
        }
    }
}

fn encode_cycle_16(
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn AudioDecoder + 'static>,
    mut encoder: FlacEncoder,
    hasher: &mut CoreWrapper<Md5Core>,
) -> Result<()> {
    let mut buffer: Vec<i32> = Vec::new();
    let track_id = format.default_track(TrackType::Audio).unwrap().id;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => match audio_buf {
                GenericAudioBufferRef::S32(buf) => {
                    let _ = buf
                        .iter_interleaved()
                        .map(|sample| {
                            let real_sample = sample >> 16;
                            hasher.update(i16::try_from(real_sample).unwrap().to_le_bytes());
                            buffer.push(real_sample);
                        })
                        .collect::<Vec<_>>();

                    encoder
                        .process_interleaved(
                            buffer.as_slice(),
                            u32::try_from(buf.samples_planar()).unwrap(),
                        )
                        .unwrap();
                    buffer.clear();
                }
                _ => return Err(anyhow!("unsupported codec")),
            },
            Err(err) => return Err(err.into()),
        }
    }

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    Ok(())
}

fn encode_cycle_24(
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn AudioDecoder + 'static>,
    mut encoder: FlacEncoder,
    hasher: &mut CoreWrapper<Md5Core>,
) -> Result<()> {
    let mut buffer: Vec<i32> = Vec::new();
    let track_id = format.default_track(TrackType::Audio).unwrap().id;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => match audio_buf {
                GenericAudioBufferRef::S32(buf) => {
                    let _ = buf
                        .iter_interleaved()
                        .map(|sample| {
                            let real_sample = sample >> 8;
                            hasher.update(i24::try_from(real_sample).unwrap().to_le_bytes());
                            buffer.push(real_sample);
                        })
                        .collect::<Vec<_>>();

                    encoder
                        .process_interleaved(
                            buffer.as_slice(),
                            u32::try_from(buf.samples_planar()).unwrap(),
                        )
                        .unwrap();
                    buffer.clear();
                }
                _ => return Err(anyhow!("unsupported codec")),
            },
            Err(err) => return Err(err.into()),
        }
    }

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    Ok(())
}

fn encode_cycle_32(
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn AudioDecoder + 'static>,
    mut encoder: FlacEncoder,
    hasher: &mut CoreWrapper<Md5Core>,
) -> Result<()> {
    let mut buffer: Vec<i32> = Vec::new();
    let track_id = format.default_track(TrackType::Audio).unwrap().id;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => match audio_buf {
                GenericAudioBufferRef::S32(buf) => {
                    let _ = buf
                        .iter_interleaved()
                        .map(|sample| {
                            hasher.update(sample.to_le_bytes());
                            buffer.push(sample);
                        })
                        .collect::<Vec<_>>();

                    encoder
                        .process_interleaved(
                            buffer.as_slice(),
                            u32::try_from(buf.samples_planar()).unwrap(),
                        )
                        .unwrap();
                    buffer.clear();
                }
                _ => return Err(anyhow!("unsupported codec")),
            },
            Err(err) => return Err(err.into()),
        }
    }

    if let Err(enc) = encoder.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    Ok(())
}

pub fn encode_file(file: &std::path::Path) -> Result<()> {
    let src = std::fs::File::open(file)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("flac");

    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();

    let format = symphonia::default::get_probe()
        .probe(&hint, mss, format_opts, metadata_opts)
        .unwrap();

    let track = format.default_track(TrackType::Audio).unwrap();

    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref().unwrap().audio().unwrap(),
            &Default::default(),
        )
        .unwrap();

    let params = track.codec_params.as_ref().unwrap().audio().unwrap();

    let config = StreamConfig {
        channels: u32::try_from(params.channels.as_ref().unwrap().count()).unwrap(),
        bits_per_sample: Bps::new(params.bits_per_sample.unwrap())?,
        sample_rate: params.sample_rate.unwrap(),
    };

    let tempname = format!("{}.tmp", file.display());

    let mut outf = File::create(&tempname)?;
    let mut outw = WriteWrapper(&mut outf);
    let enc = FlacEncoder::new()
        .unwrap()
        .channels(config.channels)
        .bits_per_sample(config.bits_per_sample.value())
        .sample_rate(config.sample_rate)
        .compression_level(8)
        .verify(false)
        .init_write(&mut outw)
        .unwrap();

    let mut hasher = Md5::new();

    match config.bits_per_sample {
        Bps::_16 => encode_cycle_16(format, decoder, enc, &mut hasher),
        Bps::_24 => encode_cycle_24(format, decoder, enc, &mut hasher),
        Bps::_32 => encode_cycle_32(format, decoder, enc, &mut hasher),
    }?;

    let tags = Tag::read_from_path(file)?;
    let mut output = Tag::read_from_path(&tempname)?;
    let mut streaminfo = tags.get_streaminfo().unwrap().clone();

    streaminfo.md5 = hasher.finalize()[..].to_vec();
    output.set_streaminfo(streaminfo);

    for block in tags.blocks() {
        match block {
            Block::VorbisComment(comment) => {
                for (key, val) in comment.comments.clone() {
                    output.set_vorbis(key, val);
                }
            }
            Block::StreamInfo(_) => {}
            _ => output.push_block(block.clone()),
        }
    }

    output.write_to_path(&tempname)?;

    std::fs::rename(&tempname, file)?;

    Ok(())
}
