//! Encoding functionality
//!
//!

use crate::common::VPXCodec;
use crate::ffi::*;

use std::mem::MaybeUninit;
use std::os::raw::c_ulong;
use std::ptr;

use av_data::frame::{Frame, FrameBufferConv, MediaKind};
use av_data::packet::Packet;
use av_data::pixel::formats::YUV420;
use av_data::pixel::Formaton;

use self::vpx_codec_err_t::*;

#[derive(Clone, Debug, PartialEq)]
pub struct PSNR {
    pub samples: [u32; 4],
    pub sse: [u64; 4],
    pub psnr: [f64; 4],
}

/// Safe wrapper around `vpx_codec_cx_pkt`
#[derive(Clone, Debug)]
pub enum VPXPacket {
    Packet(Packet),
    Stats(Vec<u8>),
    MBStats(Vec<u8>),
    PSNR(PSNR),
    Custom(Vec<u8>),
}

fn to_buffer(buf: vpx_fixed_buf_t) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(buf.sz as usize);
    unsafe {
        ptr::copy_nonoverlapping(buf.buf as *const u8, v.as_mut_ptr(), buf.sz as usize);
        v.set_len(buf.sz as usize);
    }
    v
}

impl VPXPacket {
    fn new(pkt: vpx_codec_cx_pkt) -> VPXPacket {
        use self::vpx_codec_cx_pkt_kind::*;
        match pkt.kind {
            VPX_CODEC_CX_FRAME_PKT => {
                let f = unsafe { pkt.data.frame };
                let mut p = Packet::with_capacity(f.sz as usize);
                unsafe {
                    ptr::copy_nonoverlapping(
                        f.buf as *const u8,
                        p.data.as_mut_ptr(),
                        f.sz as usize,
                    );
                    p.data.set_len(f.sz as usize);
                }
                p.t.pts = Some(f.pts);
                p.is_key = (f.flags & VPX_FRAME_IS_KEY) != 0;

                VPXPacket::Packet(p)
            }
            VPX_CODEC_STATS_PKT => {
                let b = to_buffer(unsafe { pkt.data.twopass_stats });
                VPXPacket::Stats(b)
            }
            VPX_CODEC_FPMB_STATS_PKT => {
                let b = to_buffer(unsafe { pkt.data.firstpass_mb_stats });
                VPXPacket::MBStats(b)
            }
            VPX_CODEC_PSNR_PKT => {
                let p = unsafe { pkt.data.psnr };

                VPXPacket::PSNR(PSNR {
                    samples: p.samples,
                    sse: p.sse,
                    psnr: p.psnr,
                })
            }
            VPX_CODEC_CUSTOM_PKT => {
                let b = to_buffer(unsafe { pkt.data.raw });
                VPXPacket::Custom(b)
            }
        }
    }
}

pub struct VP9EncoderConfig {
    pub cfg: vpx_codec_enc_cfg,
}

unsafe impl Send for VP9EncoderConfig {} // TODO: Make sure it cannot be abused

// TODO: Extend
fn map_formaton(img: &mut vpx_image, fmt: &Formaton) {
    use self::vpx_img_fmt_t::*;
    if fmt == YUV420 {
        img.fmt = VPX_IMG_FMT_I420;
    } else {
        unimplemented!();
    }
    img.bit_depth = 8;
    img.bps = 12;
    img.x_chroma_shift = 1;
    img.y_chroma_shift = 1;
}

fn img_from_frame(frame: &Frame) -> vpx_image {
    // This is sound because `vpx_image` is a repr(C) struct containing fields that can be
    // zeroed without causing UB
    let mut img = unsafe { MaybeUninit::zeroed().assume_init() };

    if let MediaKind::Video(ref v) = frame.kind {
        map_formaton(&mut img, &v.format);
        img.d_w = v.width as u32;
        img.d_h = v.height as u32;
    }
    // populate the buffers
    for i in 0..frame.buf.count() {
        let s: &[u8] = frame.buf.as_slice(i).unwrap();
        img.planes[i] = s.as_ptr() as *mut u8;
        img.stride[i] = frame.buf.linesize(i).unwrap() as i32;
    }

    img
}

// TODO: provide a builder?

/// VP9 Encoder setup facility
impl VP9EncoderConfig {
    /// Create a new default configuration
    pub fn new() -> Result<VP9EncoderConfig, vpx_codec_err_t> {
        let mut cfg = MaybeUninit::uninit();
        let ret = unsafe { vpx_codec_enc_config_default(vpx_codec_vp9_cx(), cfg.as_mut_ptr(), 0) };

        match ret {
            VPX_CODEC_OK => {
                let cfg = unsafe { cfg.assume_init() };
                Ok(VP9EncoderConfig { cfg })
            }
            _ => Err(ret),
        }
    }

    /// Return a newly allocated `VP9Encoder` using the current configuration
    pub fn get_encoder(&mut self) -> Result<VP9Encoder, vpx_codec_err_t> {
        VP9Encoder::new(self)
    }
}

/// VP9 Encoder
pub struct VP9Encoder {
    pub(crate) ctx: vpx_codec_ctx_t,
    pub(crate) iter: vpx_codec_iter_t,
}

unsafe impl Send for VP9Encoder {} // TODO: Make sure it cannot be abused

impl VP9Encoder {
    /// Create a new encoder using the provided configuration
    ///
    /// You may use `get_encoder` instead.
    pub fn new(cfg: &mut VP9EncoderConfig) -> Result<VP9Encoder, vpx_codec_err_t> {
        let mut ctx = MaybeUninit::uninit();
        let ret = unsafe {
            vpx_codec_enc_init_ver(
                ctx.as_mut_ptr(),
                vpx_codec_vp9_cx(),
                &cfg.cfg,
                0,
                VPX_ENCODER_ABI_VERSION as i32,
            )
        };

        match ret {
            VPX_CODEC_OK => {
                let ctx = unsafe { ctx.assume_init() };
                Ok(VP9Encoder {
                    ctx,
                    iter: ptr::null(),
                })
            }
            _ => Err(ret),
        }
    }

    /// Update the encoder parameters after-creation
    ///
    /// It calls `vpx_codec_control_`
    pub fn control(&mut self, id: vp8e_enc_control_id, val: i32) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe { vpx_codec_control_(&mut self.ctx, id as i32, val) };

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    // TODO: Cache the image information
    //
    /// Send an uncompressed frame to the encoder
    ///
    /// Call [`get_packet`] to receive the compressed data.
    ///
    /// It calls `vpx_codec_encode`.
    ///
    /// [`get_packet`]: #method.get_packet
    pub fn encode(&mut self, frame: &Frame) -> Result<(), vpx_codec_err_t> {
        let img = img_from_frame(frame);

        let ret = unsafe {
            vpx_codec_encode(
                &mut self.ctx,
                &img,
                frame.t.pts.unwrap(),
                1,
                0,
                c_ulong::from(VPX_DL_GOOD_QUALITY),
            )
        };

        self.iter = ptr::null();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    /// Notify the encoder that no more data will be sent
    ///
    /// Call [`get_packet`] to receive the compressed data.
    ///
    /// It calls `vpx_codec_encode` with NULL arguments.
    ///
    /// [`get_packet`]: #method.get_packet
    pub fn flush(&mut self) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe {
            vpx_codec_encode(
                &mut self.ctx,
                ptr::null_mut(),
                0,
                1,
                0,
                c_ulong::from(VPX_DL_GOOD_QUALITY),
            )
        };

        self.iter = ptr::null();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    /// Retrieve the compressed data
    ///
    /// To be called until it returns `None`.
    ///
    /// It calls `vpx_codec_get_cx_data`.
    pub fn get_packet(&mut self) -> Option<VPXPacket> {
        let pkt = unsafe { vpx_codec_get_cx_data(&mut self.ctx, &mut self.iter) };

        if pkt.is_null() {
            None
        } else {
            Some(VPXPacket::new(unsafe { *pkt }))
        }
    }
}

impl Drop for VP9Encoder {
    fn drop(&mut self) {
        unsafe { vpx_codec_destroy(&mut self.ctx) };
    }
}

impl VPXCodec for VP9Encoder {
    fn get_context(&mut self) -> &mut vpx_codec_ctx {
        &mut self.ctx
    }
}

#[cfg(feature = "codec-trait")]
mod encoder_trait {
    use super::*;
    use av_codec::encoder::*;
    use av_codec::error::*;
    use av_data::frame::ArcFrame;
    use av_data::params::{CodecParams, MediaKind, VideoInfo};
    use av_data::value::Value;

    pub struct Des {
        descr: Descr,
    }

    pub struct Enc {
        cfg: VP9EncoderConfig,
        enc: Option<VP9Encoder>,
    }

    impl Descriptor for Des {
        type OutputEncoder = Enc;

        fn create(&self) -> Self::OutputEncoder {
            Enc {
                cfg: VP9EncoderConfig::new().unwrap(),
                enc: None,
            }
        }

        fn describe(&self) -> &Descr {
            &self.descr
        }
    }

    impl Encoder for Enc {
        fn configure(&mut self) -> Result<()> {
            if self.enc.is_none() {
                self.cfg
                    .get_encoder()
                    .map(|enc| {
                        self.enc = Some(enc);
                    })
                    .map_err(|_err| Error::ConfigurationIncomplete)
            } else {
                unimplemented!()
            }
        }

        // TODO: have it as default impl?
        fn get_extradata(&self) -> Option<Vec<u8>> {
            None
        }

        fn send_frame(&mut self, frame: &ArcFrame) -> Result<()> {
            let enc = self.enc.as_mut().unwrap();
            enc.encode(frame).map_err(|_| unimplemented!())
        }

        fn receive_packet(&mut self) -> Result<Packet> {
            let enc = self.enc.as_mut().unwrap();

            if let Some(p) = enc.get_packet() {
                match p {
                    VPXPacket::Packet(pkt) => Ok(pkt),
                    _ => unimplemented!(),
                }
            } else {
                Err(Error::MoreDataNeeded)
            }
        }

        fn flush(&mut self) -> Result<()> {
            let enc = self.enc.as_mut().unwrap();
            enc.flush().map_err(|_| unimplemented!())
        }

        fn set_option<'a>(&mut self, key: &str, val: Value<'a>) -> Result<()> {
            match (key, val) {
                ("w", Value::U64(v)) => {
                    self.cfg.cfg.g_w = v as u32;
                    Ok(())
                }
                ("h", Value::U64(v)) => {
                    self.cfg.cfg.g_h = v as u32;
                    Ok(())
                }
                ("qmin", Value::U64(v)) => {
                    self.cfg.cfg.rc_min_quantizer = v as u32;
                    Ok(())
                }
                ("qmax", Value::U64(v)) => {
                    self.cfg.cfg.rc_max_quantizer = v as u32;
                    Ok(())
                }
                ("timebase", Value::Pair(num, den)) => {
                    self.cfg.cfg.g_timebase.num = num as i32;
                    self.cfg.cfg.g_timebase.den = den as i32;
                    Ok(())
                }
                ("lag-in-frames", Value::U64(v)) => {
                    self.cfg.cfg.g_lag_in_frames = v as u32;
                    Ok(())
                }
                ("cpu-used", Value::U64(v)) => {
                    self.enc.as_mut().ok_or(Error::InvalidData).and_then(|x| {
                        x.control(vp8e_enc_control_id::VP8E_SET_CPUUSED, v as i32)
                            .map_err(|_err| Error::ConfigurationInvalid)
                    })
                }
                ("auto-alt-ref", Value::U64(v)) => {
                    self.enc.as_mut().ok_or(Error::InvalidData).and_then(|x| {
                        x.control(vp8e_enc_control_id::VP8E_SET_ENABLEAUTOALTREF, v as i32)
                            .map_err(|_err| Error::ConfigurationInvalid)
                    })
                }
                ("arnr-maxframes", Value::U64(v)) => {
                    self.enc.as_mut().ok_or(Error::InvalidData).and_then(|x| {
                        x.control(vp8e_enc_control_id::VP8E_SET_ARNR_MAXFRAMES, v as i32)
                            .map_err(|_err| Error::ConfigurationInvalid)
                    })
                }
                ("arnr-strength", Value::U64(v)) => {
                    self.enc.as_mut().ok_or(Error::InvalidData).and_then(|x| {
                        x.control(vp8e_enc_control_id::VP8E_SET_ARNR_STRENGTH, v as i32)
                            .map_err(|_err| Error::ConfigurationInvalid)
                    })
                }
                ("arnr-type", Value::U64(v)) => {
                    self.enc.as_mut().ok_or(Error::InvalidData).and_then(|x| {
                        x.control(vp8e_enc_control_id::VP8E_SET_ARNR_TYPE, v as i32)
                            .map_err(|_err| Error::ConfigurationInvalid)
                    })
                }
                // ("format", Value::Formaton(f)) => self.format = Some(f),
                _ => unimplemented!(),
            }
        }

        fn get_params(&self) -> Result<CodecParams> {
            use std::sync::Arc;
            Ok(CodecParams {
                kind: Some(MediaKind::Video(VideoInfo {
                    height: self.cfg.cfg.g_h as usize,
                    width: self.cfg.cfg.g_w as usize,
                    format: Some(Arc::new(*YUV420)), // TODO: support more formats
                })),
                codec_id: Some("vp9".to_owned()),
                extradata: None,
                bit_rate: 0, // TODO: expose the information
                convergence_window: 0,
                delay: 0,
            })
        }

        fn set_params(&mut self, params: &CodecParams) -> Result<()> {
            if let Some(MediaKind::Video(ref info)) = params.kind {
                self.cfg.cfg.g_w = info.width as u32;
                self.cfg.cfg.g_h = info.height as u32;
            }
            Ok(())
        }
    }

    /// VP9 Encoder
    ///
    /// To be used with [av-codec](https://docs.rs/av-codec) `Encoder Context`.
    pub const VP9_DESCR: &Des = &Des {
        descr: Descr {
            codec: "vp9",
            name: "vpx",
            desc: "libvpx VP9 encoder",
            mime: "video/VP9",
        },
    };
}

#[cfg(feature = "codec-trait")]
pub use self::encoder_trait::VP9_DESCR;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn init() {
        let mut c = VP9EncoderConfig::new().unwrap();
        let mut e = c.get_encoder().unwrap();
        println!("{:#?}", c.cfg);
        println!("{}", e.error_to_str());
    }

    use super::vp8e_enc_control_id::*;
    #[test]
    fn control() {
        let mut c = VP9EncoderConfig::new().unwrap();
        c.cfg.g_w = 200;
        c.cfg.g_h = 200;
        c.cfg.g_timebase.num = 1;
        c.cfg.g_timebase.den = 1000;

        let mut e = c.get_encoder().unwrap();
        // should fail VP8-only
        let ret = e.control(VP8E_SET_TOKEN_PARTITIONS, 4);
        if let Err(err) = ret {
            println!("Ok {:?} {}", err, e.error_to_str());
        } else {
            panic!("It should fail.");
        }
        // should work common control
        e.control(VP8E_SET_CQ_LEVEL, 4).unwrap();
    }

    use av_data::rational::*;
    use av_data::timeinfo::TimeInfo;
    pub fn setup(w: u32, h: u32, t: &TimeInfo) -> VP9Encoder {
        let mut c = VP9EncoderConfig::new().unwrap();
        c.cfg.g_w = w;
        c.cfg.g_h = h;
        c.cfg.g_timebase.num = *t.timebase.unwrap().numer() as i32;
        c.cfg.g_timebase.den = *t.timebase.unwrap().denom() as i32;
        c.cfg.g_threads = 4;
        c.cfg.g_pass = vpx_enc_pass::VPX_RC_ONE_PASS;
        c.cfg.rc_end_usage = vpx_rc_mode::VPX_CQ;

        let mut e = c.get_encoder().unwrap();

        e.control(VP8E_SET_CQ_LEVEL, 4).unwrap();

        e
    }

    pub fn setup_frame(w: u32, h: u32, t: &TimeInfo) -> Frame {
        use av_data::frame::*;
        use av_data::pixel::formats;
        use std::sync::Arc;

        let v = VideoInfo::new(
            w as usize,
            h as usize,
            false,
            FrameType::OTHER,
            Arc::new(*formats::YUV420),
        );

        Frame::new_default_frame(v, Some(t.clone()))
    }

    #[test]
    fn encode() {
        let w = 200;
        let h = 200;

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Some(Rational64::new(1, 1000)),
            user_private: None,
        };

        let mut e = setup(w, h, &t);
        let mut f = setup_frame(w, h, &t);

        let mut out = 0;
        // TODO write some pattern
        for i in 0..100 {
            e.encode(&f).unwrap();
            f.t.pts = Some(i);
            println!("{:#?}", f);
            loop {
                let p = e.get_packet();

                if p.is_none() {
                    break;
                } else {
                    out = 1;
                    println!("{:#?}", p.unwrap());
                }
            }
        }

        if out != 1 {
            panic!("No packet produced");
        }
    }

    #[cfg(all(test, feature = "codec-trait"))]
    #[test]
    fn encode_codec_trait() {
        use super::VP9_DESCR;
        use av_codec::common::CodecList;
        use av_codec::encoder::*;
        use av_codec::error::*;
        use std::sync::Arc;

        let encoders = Codecs::from_list(&[VP9_DESCR]);
        let mut ctx = Context::by_name(&encoders, "vp9").unwrap();
        let w = 200;
        let h = 200;

        ctx.set_option("w", u64::from(w)).unwrap();
        ctx.set_option("h", u64::from(h)).unwrap();
        ctx.set_option("timebase", (1, 1000)).unwrap();
        ctx.set_option("qmin", 0u64).unwrap();
        ctx.set_option("qmax", 0u64).unwrap();
        ctx.set_option("lag-in-frames", 0u64).unwrap();

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Some(Rational64::new(1, 1000)),
            user_private: None,
        };

        ctx.configure().unwrap();

        ctx.set_option("cpu-used", 2u64).unwrap();
        ctx.set_option("auto-alt-ref", 1u64).unwrap();
        ctx.set_option("arnr-maxframes", 5u64).unwrap();
        ctx.set_option("arnr-strength", 3u64).unwrap();
        ctx.set_option("arnr-type", 1u64).unwrap();

        let mut f = Arc::new(setup_frame(w, h, &t));
        let mut out = 0;
        for i in 0..100 {
            Arc::get_mut(&mut f).unwrap().t.pts = Some(i);

            println!("Sending {}", i);
            ctx.send_frame(&f).unwrap();

            loop {
                match ctx.receive_packet() {
                    Ok(p) => {
                        println!("{:#?}", p);
                        out = 1
                    }
                    Err(e) => match e {
                        Error::MoreDataNeeded => break,
                        _ => unimplemented!(),
                    },
                }
            }
        }

        ctx.flush().unwrap();

        loop {
            match ctx.receive_packet() {
                Ok(p) => {
                    println!("{:#?}", p);
                    out = 1
                }
                Err(e) => match e {
                    Error::MoreDataNeeded => break,
                    _ => unimplemented!(),
                },
            }
        }

        if out != 1 {
            panic!("No packet produced");
        }
    }
}
