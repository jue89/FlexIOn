use std::{env, fs::File, io::Write, path::Path};

const fn to_kelvin(temp: f32) -> f32 {
    temp + 273.15
}

fn get_ntc(temp: f32) -> f32 {
    let ntc_r25 = {
        if let Some(str) = option_env!("NTC_R25")
            && let Ok(val) = str.parse()
        {
            val
        } else {
            10000f32
        }
    };
    let ntc_beta = {
        if let Some(str) = option_env!("NTC_BETA")
            && let Ok(val) = str.parse()
        {
            val
        } else {
            3435f32
        }
    };
    const T25: f32 = const { to_kelvin(25.0) };

    ntc_r25 * f32::exp(ntc_beta * (to_kelvin(temp).recip() - T25.recip()))
}

fn get_adc(temp: f32) -> u16 {
    let fixed_r = {
        if let Some(str) = option_env!("FIXED_R")
            && let Ok(val) = str.parse()
        {
            val
        } else {
            10000f32
        }
    };
    let ntc = get_ntc(temp);
    let frac = fixed_r / (fixed_r + ntc);
    let adc = u16::MAX as f32 * frac;
    adc.round() as _
}

fn mk_lut(values: impl Iterator<Item = i16>) -> Vec<(u16, i16)> {
    values.map(|temp| (get_adc(temp as f32), temp)).collect()
}

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("ntc_lut.rs");
    let mut file = File::create(dest_path).unwrap();
    let lut = mk_lut((0..=100).step_by(5));
    write!(
        &mut file,
        "pub const LUT: [(u16, i16); {}] = [\n",
        lut.len()
    )
    .unwrap();
    for (adc, temp) in lut {
        write!(&mut file, "    ({}, {}),\n", adc, temp).unwrap();
    }
    write!(&mut file, "];\n").unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}
