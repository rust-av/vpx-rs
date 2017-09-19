use ffi::vpx::*;

use std::ffi::CStr;
use std::mem::{ uninitialized, zeroed };
use std::mem;
use std::ptr::null_mut;
use std::rc::Rc;

use data::frame:: { Frame, MediaKind, VideoInfo, new_default_frame };
use data::pixel::formats::YUV420;

pub struct VP9Decoder {
    ctx : vpx_codec_ctx,
    iter : *mut vpx_codec_iter_t
}

use self::vpx_codec_err_t::*;

fn frame_from_img(img : vpx_image_t) -> Frame {
    use self::vpx_img_fmt_t::*;
    let f = match img.fmt {
        VPX_IMG_FMT_I420 => YUV420,
        _ => panic!("TODO: support more pixel formats")
    };
    let v = VideoInfo {
        width: img.d_w as usize,
        height: img.d_h as usize,
        format: Rc::new(*f)
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
            iter: null_mut() };
        let cfg = unsafe { zeroed() };

        let ret = unsafe { vpx_codec_dec_init_ver(&mut dec.ctx as *mut vpx_codec_ctx,
                                                  vpx_codec_vp9_dx(),
                                                  &cfg as *const vpx_codec_dec_cfg_t,
                                                  0,
                                                  VPX_DECODER_ABI_VERSION as i32) };
        match ret {
            VPX_CODEC_OK => Ok(dec),
            _ => Err(ret),
        }
    }

    pub fn error_to_str(&mut self) -> String {
        unsafe {
            let c_str = vpx_codec_error(&mut self.ctx);

            CStr::from_ptr(c_str).to_string_lossy().into_owned()
        }
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<(), vpx_codec_err_t> {
        let ret = unsafe {
            vpx_codec_decode(&mut self.ctx, data.as_ptr(),
                            data.len() as u32,
                            null_mut(),
                            0)
        };

        // Safety measure to not call get_frame on an invalid iterator
        self.iter = null_mut();

        match ret {
            VPX_CODEC_OK => Ok(()),
            _ => Err(ret)
        }
    }

    pub fn get_frame(&mut self) -> Option<Frame> {
        let img = unsafe { vpx_codec_get_frame(&mut self.ctx, self.iter) };
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn init() {
        let mut d = VP9Decoder::new().unwrap();

        println!("{}", d.error_to_str());
    }

    #[test]
    fn decode() {

    }
}
