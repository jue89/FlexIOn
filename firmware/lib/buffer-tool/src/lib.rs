#![cfg_attr(not(test), no_std)]

/// Transmutes the first `size_of::<A>()` bytes of `src` into a reference of `A`.
/// The remainder is returned as slice.
///
/// Will return `None` if `src` is smaller than `size_of::<A>()`.
pub fn snip_ref<A: Sized>(src: &[u8]) -> Option<(&A, &[u8])> {
    let len = core::mem::size_of::<A>();
    let (dst, remainder) = src.split_at_checked(len)?;
    let dst = unsafe { &*(dst.as_ptr().cast::<A>()) };
    Some((dst, remainder))
}

/// Transmutes the first `size_of::<A>()` bytes of `src` into a mutable reference of `A`.
/// The remainder is returned as slice.
///
/// Will return `None` if `src` is smaller than `size_of::<A>()`.
pub fn snip_ref_mut<A: Sized>(src: &mut [u8]) -> Option<(&mut A, &mut [u8])> {
    let len = core::mem::size_of::<A>();
    let (dst, remainder) = src.split_at_mut_checked(len)?;
    let dst = unsafe { &mut *(dst.as_mut_ptr().cast::<A>()) };
    Some((dst, remainder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snip_ref_number() {
        let buffer = [1, 2, 3, 4, 5, 6, 7, 8];
        let (num, remainder) = snip_ref::<u32>(&buffer[..]).unwrap();
        assert_eq!(*num, u32::from_le(0x04030201));
        assert_eq!(remainder, b"\x05\x06\x07\x08");
    }

    #[test]
    fn snip_ref_mut_number() {
        let mut buffer = [0u8; 8];
        let (num, remainder) = snip_ref_mut::<u32>(&mut buffer[..]).unwrap();
        *num = 0x04030201u32.to_le();
        remainder[0] = 5;
        assert_eq!(&buffer, &[1, 2, 3, 4, 5, 0, 0, 0]);
    }
}
