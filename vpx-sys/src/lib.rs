#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

include!(concat!(env!("OUT_DIR"), "/vpx.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::mem::{self, MaybeUninit};
    #[test]
    fn version() {
        println!("{}", unsafe {
            CStr::from_ptr(vpx_codec_version_str()).to_string_lossy()
        });
        println!("{}", unsafe {
            CStr::from_ptr(vpx_codec_build_config()).to_string_lossy()
        });
    }
    #[test]
    fn encode() {
        let w = 360;
        let h = 360;
        let align = 32;
        let kf_interval = 10;
        let mut raw = MaybeUninit::uninit();

        let ret = unsafe { vpx_img_alloc(raw.as_ptr(), vpx_img_fmt::VPX_IMG_FMT_I420, w, h, align) };
        if ret.is_null() {
            panic!("Image allocation failed");
        }
        mem::forget(ret); // raw and ret are the same
        print!("{:#?}", raw);

        let mut cfg = MaybeUninit::uninit();
        let mut ret = unsafe { vpx_codec_enc_config_default(vpx_codec_vp9_cx(), cfg.as_mut_ptr(), 0) };

        if ret != vpx_codec_err_t::VPX_CODEC_OK {
            panic!("Default Configuration failed");
        }

        cfg.g_w = w;
        cfg.g_h = h;
        cfg.g_timebase.num = 1;
        cfg.g_timebase.den = 30;
        cfg.rc_target_bitrate = 100 * 1014;

        let mut ctx = MaybeUninit::uninit();
        ret = unsafe {
            vpx_codec_enc_init_ver(
                ctx.as_mut_ptr(),
                vpx_codec_vp9_cx(),
                &mut cfg,
                0,
                VPX_ENCODER_ABI_VERSION as i32,
            )
        };

        if ret != vpx_codec_err_t::VPX_CODEC_OK {
            panic!("Codec Init failed");
        }

        let mut out = 0;
        for i in 0..100 {
            let mut flags = 0;
            if i % kf_interval == 0 {
                flags |= VPX_EFLAG_FORCE_KF;
            }
            unsafe {
                let ret = vpx_codec_encode(
                    ctx.as_mut_ptr(),
                    raw.as_mut_ptr(),
                    i,
                    1,
                    flags as i64,
                    VPX_DL_GOOD_QUALITY as u64,
                );
                if ret != vpx_codec_err_t::VPX_CODEC_OK {
                    panic!("Encode failed {:?}", ret);
                }

                let mut iter = MaybeUninit::zeroed();
                loop {
                    let pkt = vpx_codec_get_cx_data(ctx.as_mut_ptr(), iter.as_mut_ptr());

                    if pkt.is_null() {
                        break;
                    } else {
                        println!("{:#?}", (*pkt).kind);
                        out = 1;
                    }
                }
            }
        }

        if out != 1 {
            panic!("No packet produced");
        }
    }
}
