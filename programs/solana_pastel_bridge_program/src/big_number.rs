/// Trait for calculating `val * num / denom` with different rounding modes and overflow
/// protection.
///
/// Implementations of this trait have to ensure that even if the result of the multiplication does
/// not fit into the type, as long as it would fit after the division the correct result has to be
/// returned instead of `None`. `None` only should be returned if the overall result does not fit
/// into the type.
///
/// This specifically means that e.g. the `u64` implementation must, depending on the arguments, be
/// able to do 128 bit integer multiplication.
pub trait CheckedMulDiv<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    /// Calculates `floor(val * num / denom)`, i.e. the largest integer less than or equal to the
    /// result of the division.
    fn checked_mul_div_down(self, num: RHS, denom: RHS) -> Option<Self::Output>;

    /// Calculates `ceil(val * num / denom)`, i.e. the the smallest integer greater than or equal to
    /// the result of the division.
    fn checked_mul_div_up(self, num: RHS, denom: RHS) -> Option<Self::Output>;
}

pub trait CheckedDivCeil<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    /// Calculates `ceil(val / denom)`, i.e. the the smallest integer greater than or equal to
    /// the result of the division.
    fn checked_div_up(self, denom: RHS) -> Option<Self::Output>;
}

pub trait CheckedDivFloor<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    /// Calculates `floor(val / denom)`, i.e. the largest integer less than or equal to the
    /// result of the division.
    fn checked_div_down(self, denom: RHS) -> Option<Self::Output>;
}

impl CheckedMulDiv for u64 {
    type Output = u64;

    fn checked_mul_div_down(self, num: Self, denom: Self) -> Option<Self::Output> {
        assert_ne!(denom, 0);
        let r = (self as u128 * num as u128) / denom as u128;
        if r > u64::MAX as u128 {
            None
        } else {
            Some(r as u64)
        }
    }

    fn checked_mul_div_up(self, num: Self, denom: Self) -> Option<Self::Output> {
        assert_ne!(denom, 0);
        let r = (self as u128 * num as u128 + denom.saturating_sub(1) as u128) / denom as u128;
        if r > u64::MAX as u128 {
            None
        } else {
            Some(r as u64)
        }
    }
}

impl CheckedDivCeil for u64 {
    type Output = u64;

    fn checked_div_up(self, denom: Self) -> Option<Self::Output> {
        assert_ne!(denom, 0);
        let r = (self as u128 + denom.saturating_sub(1) as u128) / denom as u128;
        if r > u64::MAX as u128 {
            None
        } else {
            Some(r as u64)
        }
    }
}
