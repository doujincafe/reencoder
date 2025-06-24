mod decoder;

use anyhow::{Result, anyhow};
use flac_bound::{FlacEncoder, WriteWrapper};
/* use md5::Digest; */
use metaflac::Tag;
use std::{fs::File, path::Path};

use crate::flac::decoder::FlacDecoder;

pub const CURRENT_VENDOR: &str = "reference libFLAC 1.5.0 20250211";

/* type BoxedFormatReader = Box<dyn FormatReader>;
type BoxedAudioDecoder = Box<dyn AudioDecoder + 'static>; */

/* pub struct StreamConfig {
    channels: u32,
    bits_per_sample: Bps,
    sample_rate: u32,
}

pub enum Bps {
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
} */

/* struct FileEncoder {
    filename: PathBuf,
    streamdata: StreamConfig,
    format: BoxedFormatReader,
    decoder: BoxedAudioDecoder,
}

impl FileEncoder {
    fn new(file: impl AsRef<Path>) -> Result<Self> {
        let (format, decoder, config) = init_decoder(&file)?;
        Ok(FileEncoder {
            filename: file.as_ref().to_path_buf(),
            streamdata: config,
            format,
            decoder,
        })
    }

    fn temp_name(&self) -> PathBuf {
        self.filename.clone().with_extension("tmp")
    }

    fn encode(&mut self, mut encoder: FlacEncoder) -> Result<Vec<u8>> {
        let mut buffer: Vec<i32> = Vec::new();
        let mut hasher = Md5::new();
        let track_id = self.format.default_track(TrackType::Audio).unwrap().id;
        let offset = self.streamdata.bits_per_sample.value();

        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(error) => return Err(error.into()),
            };

            if packet.track_id() != track_id {
                continue;
            }

            if let GenericAudioBufferRef::S32(buf) = self.decoder.decode(&packet)? {
                for sample in buf.iter_interleaved() {
                    let mut real_sample = sample;
                    if offset != 32 {
                        real_sample = sample >> (32 - offset)
                    }
                    match offset {
                        16 => hasher.update(i16::try_from(real_sample)?.to_le_bytes()),
                        24 => hasher
                            .update(i24::i24::try_from_i32(real_sample).unwrap().to_le_bytes()),
                        _ => hasher.update(real_sample.to_le_bytes()),
                    }
                    buffer.push(real_sample);
                }
                encoder
                    .process_interleaved(&buffer, buf.samples_planar() as u32)
                    .unwrap();
                buffer.clear();
            } else {
                return Err(anyhow!("unsupported codec"));
            }
        }

        if let Err(enc) = encoder.finish() {
            return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
        }

        Ok(hasher.finalize().to_vec())
    }

    fn write_tags(&self, hash: Vec<u8>) -> Result<()> {
        let tags = Tag::read_from_path(&self.filename)?;
        let mut output = Tag::read_from_path(self.temp_name())?;

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

        output.write_to_path(self.temp_name())?;
        Ok(())
    }
}

fn init_decoder(
    filename: impl AsRef<Path>,
) -> Result<(BoxedFormatReader, BoxedAudioDecoder, StreamConfig)> {
    let src = std::fs::File::open(filename)?;
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

    Ok((format, decoder, config))
} */

pub fn encode_file(filename: impl AsRef<Path>) -> Result<()> {
    /* let mut filencoder = FileEncoder::new(filename)?;
    let temp_name = filencoder.temp_name();

    if temp_name.exists() {
        std::fs::remove_file(&temp_name)?;
    }

    let mut outf = File::create(temp_name)?;
    let mut outw = WriteWrapper(&mut outf);
    let enc = FlacEncoder::new()
        .unwrap()
        .channels(filencoder.streamdata.channels)
        .bits_per_sample(filencoder.streamdata.bits_per_sample.value())
        .sample_rate(filencoder.streamdata.sample_rate)
        .compression_level(8)
        .verify(false)
        .init_write(&mut outw)
        .unwrap();

    let hash = filencoder.encode(enc)?;
    filencoder.write_tags(hash)?;
    std::fs::rename(filencoder.temp_name(), filencoder.filename)?;
    Ok(()) */

    let decoder = FlacDecoder::new();
    let mut buffer: Vec<i32> = Vec::new();
    decoder.init_decode_from_file(&filename, &mut buffer)?;

    let temp_name = filename.as_ref().with_extension("tmp");
    let mut outf = File::create(temp_name)?;
    let mut outw = WriteWrapper(&mut outf);
    let mut enc = FlacEncoder::new()
        .unwrap()
        .channels(decoder.get_channels())
        .bits_per_sample(decoder.get_bps())
        .sample_rate(decoder.get_samplerate())
        .compression_level(8)
        .verify(false)
        .init_write(&mut outw)
        .unwrap();

    while decoder.decode_frame().is_ok() {
        enc.process_interleaved(&buffer, buffer.iter().len() as u32 / decoder.get_channels())
            .unwrap();
        buffer.clear();
    }

    todo!()
}

pub fn get_vendor(file: impl AsRef<Path>) -> Result<String> {
    if let Some(vorbis) = Tag::read_from_path(file)?.vorbis_comments() {
        Ok(vorbis.vendor_string.to_owned())
    } else {
        Err(anyhow!("Vendor string not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaflac::Tag;

    #[test]
    fn bit16() {
        let name = "16bit.flac";
        let tempname = "16bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = Tag::read_from_path(tempname)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path(name)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(name).unwrap();
        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, encoded_md5);
    }

    #[test]
    fn bit24() {
        let name = "24bit.flac";
        let tempname = "24bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = Tag::read_from_path(tempname)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path(name)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(name).unwrap();
        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, encoded_md5);
    }

    #[test]
    fn bit32() {
        let name = "32bit.flac";
        let tempname = "32bit.flac.temp";
        std::fs::copy(name, tempname).unwrap();
        encode_file(name).unwrap();
        let target_md5 = Tag::read_from_path(tempname)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        let encoded_md5 = Tag::read_from_path(name)
            .unwrap()
            .get_streaminfo()
            .unwrap()
            .md5
            .clone();
        std::fs::remove_file(name).unwrap();
        std::fs::rename(tempname, name).unwrap();
        assert_eq!(target_md5, encoded_md5);
    }
}
