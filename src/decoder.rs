//! Decoding functionality
//!
//!

use crate::common::VPXCodec;
use crate::ffi::*;

use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Arc;

use crate::data::frame::{new_default_frame, FrameType};
use crate::data::frame::{Frame, VideoInfo};
use crate::data::pixel::formats::YUV420;

use self::vpx_codec_err_t::*;

fn frame_from_img(img: vpx_image_t) -> Frame {
    use self::vpx_img_fmt_t::*;
    let f = match img.fmt {
        VPX_IMG_FMT_I420 => YUV420,
        _ => panic!("TODO: support more pixel formats"),
    };
    let v = VideoInfo::new(
        img.d_w as usize,
        img.d_h as usize,
        false,
        FrameType::OTHER,
        Arc::new(*f),
    );

    let mut f = new_default_frame(v, None);

    let src = img
        .planes
        .iter()
        .zip(img.stride.iter())
        .map(|(v, l)| unsafe { std::slice::from_raw_parts(*v as *const u8, *l as usize) });

    let linesize = img.stride.iter().map(|l| *l as usize);

    f.copy_from_slice(src, linesize);
    f
}

use std::marker::PhantomData;

/// VP9 Decoder
pub struct VP9Decoder<T> {
    pub(crate) ctx: vpx_codec_ctx,
    pub(crate) iter: vpx_codec_iter_t,
    private_data: PhantomData<T>,
}

unsafe impl<T: Send> Send for VP9Decoder<T> {} // TODO: Make sure it cannot be abused

impl<T> VP9Decoder<T> {
    /// Create a new decoder
    ///
    /// # Errors
    ///
    /// The function may fail if the underlying libvpx does not provide
    /// the VP9 decoder.
    pub fn new() -> Result<VP9Decoder<T>, vpx_codec_err_t> {
        let mut ctx = MaybeUninit::uninit();
        let cfg = MaybeUninit::zeroed();

        let ret = unsafe {
            vpx_codec_dec_init_ver(
                ctx.as_mut_ptr(),
                vpx_codec_vp9_dx(),
                cfg.as_ptr(),
                0,
                VPX_DECODER_ABI_VERSION as i32,
            )
        };

        match ret {
            VPX_CODEC_OK => {
                let ctx = unsafe { ctx.assume_init() };
                Ok(VP9Decoder {
                    ctx,
                    iter: ptr::null(),
                    private_data: PhantomData,
                })
            },
            _ => Err(ret),
        }
    }

    /// Feed some compressed data to the encoder
    ///
    /// The `data` slice is sent to the decoder alongside the optional
    /// `private` struct.
    ///
    /// The [`get_frame`] method must be called to retrieve the decompressed
    /// frame, do not call this method again before calling [`get_frame`].
    ///
    /// It matches a call to `vpx_codec_decode`.
    ///
    /// [`get_frame`]: #method.get_frame
    pub fn decode<O>(&mut self, data: &[u8], private: O) -> Result<(), vpx_codec_err_t>
    where
        O: Into<Option<T>>,
    {
        let priv_data = private
            .into()
            .map(|v| Box::into_raw(Box::new(v)))
            .unwrap_or(ptr::null_mut());
        let ret = unsafe {
            vpx_codec_decode(
                &mut self.ctx,
                data.as_ptr(),
                data.len() as u32,
                priv_data as *mut std::ffi::c_void,
                0,
            )
        };

        // Safety measure to not call get_frame on an invalid iterator
        self.iter = ptr::null();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => {
                let _ = unsafe { Box::from_raw(priv_data) };
                Err(ret)
            }
        }
    }

    /// Notify the decoder to return any pending frame
    ///
    /// The [`get_frame`] method must be called to retrieve the decompressed
    /// frame.
    ///
    /// It matches a call to `vpx_codec_decode` with NULL arguments.
    ///
    /// [`get_frame`]: #method.get_frame
    pub fn flush(&mut self) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe { vpx_codec_decode(&mut self.ctx, ptr::null(), 0, ptr::null_mut(), 0) };

        self.iter = ptr::null();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    /// Retrieve decoded frames
    ///
    /// Should be called repeatedly until it returns `None`.
    ///
    /// It matches a call to `vpx_codec_get_frame`.
    pub fn get_frame(&mut self) -> Option<(Frame, Option<Box<T>>)> {
        let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut self.iter) };
        if img.is_null() {
            None
        } else {
            let im = unsafe { *img };
            let priv_data = if im.user_priv.is_null() {
                None
            } else {
                let p = im.user_priv as *mut T;
                Some(unsafe { Box::from_raw(p) })
            };
            let frame = frame_from_img(im);
            Some((frame, priv_data))
        }
    }
}

impl<T> Drop for VP9Decoder<T> {
    fn drop(&mut self) {
        unsafe { vpx_codec_destroy(&mut self.ctx) };
    }
}

impl<T> VPXCodec for VP9Decoder<T> {
    fn get_context(&mut self) -> &mut vpx_codec_ctx {
        &mut self.ctx
    }
}

#[cfg(feature = "codec-trait")]
mod decoder_trait {
    use super::*;
    use crate::codec::decoder::*;
    use crate::codec::error::*;
    use crate::data::frame::ArcFrame;
    use crate::data::packet::Packet;
    use crate::data::timeinfo::TimeInfo;
    use std::sync::Arc;

    struct Des {
        descr: Descr,
    }

    impl Descriptor for Des {
        fn create(&self) -> Box<dyn Decoder> {
            Box::new(VP9Decoder::new().unwrap())
        }

        fn describe(&self) -> &Descr {
            &self.descr
        }
    }

    impl Decoder for VP9Decoder<TimeInfo> {
        fn set_extradata(&mut self, _extra: &[u8]) {
            // No-op
        }
        fn send_packet(&mut self, pkt: &Packet) -> Result<()> {
            self.decode(&pkt.data, pkt.t.clone())
                .map_err(|_err| unimplemented!())
        }
        fn receive_frame(&mut self) -> Result<ArcFrame> {
            self.get_frame()
                .map(|(mut f, t)| {
                    f.t = t.map(|b| *b).unwrap();
                    Arc::new(f)
                })
                .ok_or(Error::MoreDataNeeded)
        }
        fn flush(&mut self) -> Result<()> {
            self.flush().map_err(|_err| unimplemented!())
        }
        fn configure(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// VP9 Decoder
    ///
    /// To be used with [av-codec](https://docs.rs/av-codec) `Context`.
    pub const VP9_DESCR: &dyn Descriptor = &Des {
        descr: Descr {
            codec: "vp9",
            name: "vpx",
            desc: "libvpx VP9 decoder",
            mime: "video/VP9",
        },
    };
}

#[cfg(feature = "codec-trait")]
pub use self::decoder_trait::VP9_DESCR;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn init() {
        let mut d = VP9Decoder::<()>::new().unwrap();

        println!("{}", d.error_to_str());
    }

    use super::super::encoder::tests as enc;
    use super::super::encoder::VPXPacket;
    use crate::data::rational::*;
    use crate::data::timeinfo::TimeInfo;
    #[test]
    fn decode() {
        let w = 800;
        let h = 600;

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Some(Rational64::new(1, 1000)),
            user_private: None,
        };

        let mut e = enc::setup(w, h, &t);
        let mut f = enc::setup_frame(w, h, &t);

        let mut d = VP9Decoder::<()>::new().unwrap();
        let mut out = 0;

        for i in 0..100 {
            e.encode(&f).unwrap();
            f.t.pts = Some(i);

            println!("{:#?}", f);
            loop {
                let p = e.get_packet();

                if p.is_none() {
                    break;
                } else if let VPXPacket::Packet(ref pkt) = p.unwrap() {
                    d.decode(&pkt.data, None).unwrap();

                    // No multiframe expected.
                    if let Some(f) = d.get_frame() {
                        out = 1;
                        println!("{:#?}", f);
                    }
                }
            }
        }

        if out != 1 {
            panic!("No frame decoded");
        }
    }

    #[cfg(all(test, feature = "codec-trait"))]
    #[test]
    fn decode_codec_trait() {
        use super::super::decoder::VP9_DESCR as DEC;
        use super::super::encoder::VP9_DESCR as ENC;
        use crate::codec::common::CodecList;
        use crate::codec::decoder as de;
        use crate::codec::encoder as en;
        use crate::codec::error::*;
        use std::sync::Arc;

        let encoders = en::Codecs::from_list(&[ENC]);
        let decoders = de::Codecs::from_list(&[DEC]);
        let mut enc = en::Context::by_name(&encoders, "vp9").unwrap();
        let mut dec = de::Context::by_name(&decoders, "vp9").unwrap();
        let w = 200;
        let h = 200;

        enc.set_option("w", u64::from(w)).unwrap();
        enc.set_option("h", u64::from(h)).unwrap();
        enc.set_option("timebase", (1, 1000)).unwrap();

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Some(Rational64::new(1, 1000)),
            user_private: None,
        };

        enc.configure().unwrap();
        dec.configure().unwrap();

        let mut f = Arc::new(enc::setup_frame(w, h, &t));
        let mut enc_out = 0;
        let mut dec_out = 0;
        for i in 0..100 {
            Arc::get_mut(&mut f).unwrap().t.pts = Some(i);

            println!("Sending {}", i);
            enc.send_frame(&f).unwrap();

            loop {
                match enc.receive_packet() {
                    Ok(p) => {
                        println!("{:#?}", p);
                        enc_out = 1;
                        dec.send_packet(&p).unwrap();

                        loop {
                            match dec.receive_frame() {
                                Ok(f) => {
                                    println!("{:#?}", f);
                                    dec_out = 1;
                                }
                                Err(e) => match e {
                                    Error::MoreDataNeeded => break,
                                    _ => unimplemented!(),
                                },
                            }
                        }
                    }
                    Err(e) => match e {
                        Error::MoreDataNeeded => break,
                        _ => unimplemented!(),
                    },
                }
            }
        }

        enc.flush().unwrap();

        loop {
            match enc.receive_packet() {
                Ok(p) => {
                    println!("{:#?}", p);
                    enc_out = 1
                }
                Err(e) => match e {
                    Error::MoreDataNeeded => break,
                    _ => unimplemented!(),
                },
            }
        }

        if enc_out != 1 || dec_out != 1 {
            panic!();
        }
    }
}
