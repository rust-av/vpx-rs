use common::VPXCodec;
use ffi::vpx::*;

use std::mem::{uninitialized, zeroed};
use std::mem;
use std::ptr;
use std::rc::Rc;

use data::frame::{Frame, MediaKind, VideoInfo};
use data::frame::{PictureType, new_default_frame};
use data::pixel::formats::YUV420;

pub struct VP9Decoder {
    ctx: vpx_codec_ctx,
    iter: vpx_codec_iter_t,
}

use self::vpx_codec_err_t::*;

fn frame_from_img(img: vpx_image_t) -> Frame {
    use self::vpx_img_fmt_t::*;
    let f = match img.fmt {
        VPX_IMG_FMT_I420 => YUV420,
        _ => panic!("TODO: support more pixel formats"),
    };
    let v = VideoInfo {
        pic_type: PictureType::UNKNOWN,
        width: img.d_w as usize,
        height: img.d_h as usize,
        format: Rc::new(*f),
    };

    let mut f = new_default_frame(&MediaKind::Video(v), None);

    let src = img.planes.iter().map(|v| *v as *const u8);
    let linesize = img.stride.iter().map(|l| *l as usize);

    f.copy_from_raw_parts(src, linesize);
    f
}

impl VP9Decoder {
    pub fn new() -> Result<VP9Decoder, vpx_codec_err_t> {
        let mut dec = VP9Decoder {
            ctx: unsafe { uninitialized() },
            iter: ptr::null(),
        };
        let cfg = unsafe { zeroed() };

        let ret = unsafe {
            vpx_codec_dec_init_ver(
                &mut dec.ctx as *mut vpx_codec_ctx,
                vpx_codec_vp9_dx(),
                &cfg as *const vpx_codec_dec_cfg_t,
                0,
                VPX_DECODER_ABI_VERSION as i32,
            )
        };
        match ret {
            VPX_CODEC_OK => Ok(dec),
            _ => Err(ret),
        }
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe {
            vpx_codec_decode(
                &mut self.ctx,
                data.as_ptr(),
                data.len() as u32,
                ptr::null_mut(),
                0,
            )
        };

        // Safety measure to not call get_frame on an invalid iterator
        self.iter = ptr::null();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret),
        }
    }

    pub fn get_frame(&mut self) -> Option<Frame> {
        let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut self.iter) };
        mem::forget(img);

        if img.is_null() {
            None
        } else {
            let frame = frame_from_img(unsafe { *img });
            Some(frame)
        }
    }
}

impl Drop for VP9Decoder {
    fn drop(&mut self) {
        unsafe { vpx_codec_destroy(&mut self.ctx) };
    }
}

impl VPXCodec for VP9Decoder {
    fn get_context<'a>(&'a mut self) -> &'a mut vpx_codec_ctx {
        &mut self.ctx
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn init() {
        let mut d = VP9Decoder::new().unwrap();

        println!("{}", d.error_to_str());
    }

    use super::super::encoder::tests as enc;
    use super::super::encoder::VPXPacket;
    use data::timeinfo::TimeInfo;
    use data::rational::*;
    #[test]
    fn decode() {
        let w = 800;
        let h = 600;

        let t = TimeInfo {
            pts: Some(0),
            dts: Some(0),
            duration: Some(1),
            timebase: Rational32::new(1, 1000),
        };

        let mut e = enc::setup(w, h, &t);
        let mut f = enc::setup_frame(w, h, &t);

        let mut d = VP9Decoder::new().unwrap();
        let mut out = 0;

        for i in 0..100 {
            e.encode(&f).unwrap();
            if let Some(ref mut t) = f.t {
                t.pts = Some(i);
            }
            println!("{:#?}", f);
            loop {
                let p = e.get_packet();

                if p.is_none() {
                    break;
                } else {
                    if let VPXPacket::Packet(ref pkt) = p.unwrap() {
                        let _ = d.decode(&pkt.data).unwrap();

                        // No multiframe expected.
                        if let Some(f) = d.get_frame() {
                            out = 1;
                            println!("{:#?}", f);
                        }
                    }
                }
            }
        }

        if out != 1 {
            panic!("No frame decoded");
        }
    }
}
