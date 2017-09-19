use common::VPXCodec;
use ffi::vpx::*;

use std::mem;

use data::frame::{Frame, MediaKind};
use data::pixel::Formaton;
use data::pixel::formats::YUV420;

use self::vpx_codec_err_t::*;

pub struct VP9EncoderConfig {
    pub cfg: vpx_codec_enc_cfg,
}

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

fn img_from_frame<'a>(frame: &'a Frame) -> vpx_image {
    let mut img: vpx_image = unsafe { mem::zeroed() };

    if let MediaKind::Video(ref v) = frame.kind {
        map_formaton(&mut img, &v.format);
        img.d_w = v.width as u32;
        img.d_h = v.height as u32;
    }
    // populate the buffers
    for i in 0..frame.buf.count() {
        img.planes[i] = unsafe { mem::transmute(frame.buf.as_slice(i).unwrap().as_ptr()) };
        img.stride[i] = frame.buf.linesize(i).unwrap() as i32;
    }

    img
}

// TODO: provide a builder?
impl VP9EncoderConfig {
    pub fn new() -> Result<VP9EncoderConfig, vpx_codec_err_t> {
        let mut cfg = unsafe { mem::uninitialized() };
        let ret = unsafe { vpx_codec_enc_config_default(vpx_codec_vp9_cx(), &mut cfg, 0) };

        match ret {
            VPX_CODEC_OK => Ok(VP9EncoderConfig { cfg: cfg }),
            _ => Err(ret),
        }
    }

    pub fn get_encoder(&mut self) -> Result<VP9Encoder, vpx_codec_err_t> {
        VP9Encoder::new(self)
    }
}

pub struct VP9Encoder {
    ctx: vpx_codec_ctx_t,
}

impl VP9Encoder {
    pub fn new(cfg: &mut VP9EncoderConfig) -> Result<VP9Encoder, vpx_codec_err_t> {
        let mut ctx = unsafe { mem::uninitialized() };
        let ret = unsafe {
            vpx_codec_enc_init_ver(
                &mut ctx,
                vpx_codec_vp9_cx(),
                &mut cfg.cfg,
                0,
                VPX_ENCODER_ABI_VERSION as i32,
            )
        };

        match ret {
            VPX_CODEC_OK => Ok(VP9Encoder { ctx: ctx }),
            _ => Err(ret),
        }
    }

    pub fn control(&mut self, id: vp8e_enc_control_id, val: i32) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe { vpx_codec_control_(&mut self.ctx, id as i32, val) };

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    // TODO: Cache the image information
    pub fn encode(&mut self, frame: Frame) -> Result<(), vpx_codec_err_t> {
        let mut img = img_from_frame(&frame);

        let ret = unsafe {
            vpx_codec_encode(
                &mut self.ctx,
                &mut img,
                frame.t.unwrap().pts.unwrap(),
                1,
                0,
                VPX_DL_GOOD_QUALITY as u64,
            )
        };

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    pub fn get_packet() {}
}

impl Drop for VP9Encoder {
    fn drop(&mut self) {
        unsafe { vpx_codec_destroy(&mut self.ctx) };
    }
}

impl VPXCodec for VP9Encoder {
    fn get_context<'a>(&'a mut self) -> &'a mut vpx_codec_ctx {
        &mut self.ctx
    }
}

#[cfg(test)]
mod tests {
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
    #[test]
    fn encode() {
        use data::frame::*;
        use data::pixel::formats;
        use data::timeinfo::TimeInfo;
        use data::rational::*;
        use std::rc::Rc;

        let w = 200;
        let h = 200;

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Rational32::new(1, 1000),
        };

        let mut c = VP9EncoderConfig::new().unwrap();
        c.cfg.g_w = w;
        c.cfg.g_h = h;
        c.cfg.g_timebase.num = *t.timebase.numer();
        c.cfg.g_timebase.den = *t.timebase.denom();

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

        let v = VideoInfo {
            pic_type: PictureType::UNKNOWN,
            width: w as usize,
            height: h as usize,
            format: Rc::new(*formats::YUV420),
        };

        let mut f = new_default_frame(&MediaKind::Video(v), Some(t));

        // TODO write some pattern
        e.encode(f).unwrap();
    }
}
