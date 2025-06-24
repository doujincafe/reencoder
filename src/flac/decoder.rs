use std::{
    error::Error,
    ffi::{CString, c_void},
    fmt::Display,
    path::Path,
};

use libflac_sys::{
    FLAC__STREAM_DECODER_INIT_STATUS_ALREADY_INITIALIZED,
    FLAC__STREAM_DECODER_INIT_STATUS_ERROR_OPENING_FILE,
    FLAC__STREAM_DECODER_INIT_STATUS_INVALID_CALLBACKS,
    FLAC__STREAM_DECODER_INIT_STATUS_MEMORY_ALLOCATION_ERROR, FLAC__STREAM_DECODER_INIT_STATUS_OK,
    FLAC__STREAM_DECODER_INIT_STATUS_UNSUPPORTED_CONTAINER, FLAC__StreamDecoder,
    FLAC__StreamDecoderInitStatus, FLAC__bool, FLAC__stream_decoder_finish,
    FLAC__stream_decoder_get_bits_per_sample, FLAC__stream_decoder_get_channels,
    FLAC__stream_decoder_get_sample_rate, FLAC__stream_decoder_init_file, FLAC__stream_decoder_new,
    FLAC__stream_decoder_process_single, FLAC__stream_decoder_set_md5_checking,
};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FlacDecoderInitError {
    AlreadyInitialized = FLAC__STREAM_DECODER_INIT_STATUS_ALREADY_INITIALIZED,

    ErrorOpeningFile = FLAC__STREAM_DECODER_INIT_STATUS_ERROR_OPENING_FILE,

    InvalidCallbacks = FLAC__STREAM_DECODER_INIT_STATUS_INVALID_CALLBACKS,

    MemoryAllocationError = FLAC__STREAM_DECODER_INIT_STATUS_MEMORY_ALLOCATION_ERROR,

    UnsupportedContainer = FLAC__STREAM_DECODER_INIT_STATUS_UNSUPPORTED_CONTAINER,
}

impl Error for FlacDecoderInitError {}

impl Display for FlacDecoderInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<FlacDecoderInitError> for FLAC__StreamDecoderInitStatus {
    fn from(val: FlacDecoderInitError) -> Self {
        val as FLAC__StreamDecoderInitStatus
    }
}

impl TryFrom<FLAC__StreamDecoderInitStatus> for FlacDecoderInitError {
    type Error = ();

    #[allow(non_upper_case_globals)]
    fn try_from(raw: FLAC__StreamDecoderInitStatus) -> Result<FlacDecoderInitError, ()> {
        Ok(match raw {
            FLAC__STREAM_DECODER_INIT_STATUS_ALREADY_INITIALIZED => {
                FlacDecoderInitError::AlreadyInitialized
            }
            FLAC__STREAM_DECODER_INIT_STATUS_ERROR_OPENING_FILE => {
                FlacDecoderInitError::ErrorOpeningFile
            }
            FLAC__STREAM_DECODER_INIT_STATUS_INVALID_CALLBACKS => {
                FlacDecoderInitError::InvalidCallbacks
            }
            FLAC__STREAM_DECODER_INIT_STATUS_MEMORY_ALLOCATION_ERROR => {
                FlacDecoderInitError::MemoryAllocationError
            }
            FLAC__STREAM_DECODER_INIT_STATUS_UNSUPPORTED_CONTAINER => {
                FlacDecoderInitError::UnsupportedContainer
            }
            _ => return Err(()),
        })
    }
}

fn convert_path(path: &Path) -> CString {
    CString::new(path.to_str().expect("non-UTF-8 filename")).expect("filename has internal NULs")
}

pub(crate) struct FlacDecoder(pub *mut FLAC__StreamDecoder);

impl FlacDecoder {
    pub(crate) fn new() -> Self {
        FlacDecoder(unsafe { FLAC__stream_decoder_new() })
    }

    pub(crate) fn init_decode_from_file<P: AsRef<Path>>(
        &self,
        file: &P,
        buf: &mut Vec<i32>,
    ) -> Result<(), FlacDecoderInitError> {
        unsafe {
            FLAC__stream_decoder_set_md5_checking(self.0, true as FLAC__bool);
        }

        let filename = convert_path(file.as_ref());
        unsafe {
            let result: FLAC__StreamDecoderInitStatus = FLAC__stream_decoder_init_file(
                self.0,
                filename.as_ptr(),
                None,
                None,
                None,
                buf.as_mut_ptr() as *mut c_void,
            );
            if result != FLAC__STREAM_DECODER_INIT_STATUS_OK {
                return Err(FlacDecoderInitError::try_from(result).unwrap());
            }
        }

        Ok(())
    }

    pub(crate) fn decode_frame(&self) -> Result<(), ()> {
        if unsafe { FLAC__stream_decoder_process_single(self.0) } != 0 {
            Ok(())
        } else {
            Err(())
        }
    }

    pub(crate) fn get_channels(&self) -> u32 {
        unsafe { FLAC__stream_decoder_get_channels(self.0) }
    }

    pub(crate) fn get_bps(&self) -> u32 {
        unsafe { FLAC__stream_decoder_get_bits_per_sample(self.0) }
    }

    pub(crate) fn get_samplerate(&self) -> u32 {
        unsafe { FLAC__stream_decoder_get_sample_rate(self.0) }
    }
}

impl Drop for FlacDecoder {
    fn drop(&mut self) {
        if !(self.0.is_null()) {
            unsafe { FLAC__stream_decoder_finish(self.0) };
        }
    }
}
