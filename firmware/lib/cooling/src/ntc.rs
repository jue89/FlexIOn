use physical_values::Temperature;

include!(concat!(env!("OUT_DIR"), "/ntc_lut.rs"));
pub fn adc_to_temp(adc: u16) -> Option<Temperature> {
    let (idx, (adc1, temp1)) = LUT.iter().enumerate().find(|(_, (a, _))| *a > adc)?;
    if idx == 0 {
        return None;
    }
    let (adc0, temp0) = &LUT[idx - 1];
    let temp_diff = (*temp1 - *temp0) * 100;
    let adc_diff = (*adc1 - *adc0) as i16;
    let slope = adc_diff / temp_diff;
    let temp = (adc - *adc0) as i16 / slope + *temp0 * 100;
    Some(Temperature::from_decimal(temp as i32, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert() {
        assert_eq!(adc_to_temp(u16::MIN), None);
        assert_eq!(adc_to_temp(u16::MAX), None);
        assert_eq!(adc_to_temp(u16::MAX / 2), Some(Temperature::from_val(25)));
    }
}
